//! Cross-function calls, property access, and method invocations for CLIF allocation lowering.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;

use super::super::emit::{
    box_or_pass, call_helper, call_helper_void, meta_is_float, unbox_bool, unbox_f64_coerce,
    use_f64, use_int,
};
use super::super::kinds::K;
use super::safepoints::{
    def_result, flush_boxed, frame_base_addr, live_boxed, reload_boxed, store_home, AllocCtx,
};

pub(crate) fn emit_call(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    target: Option<&crate::clif::lower::ClifTarget>,
) -> Result<(), String> {
    let w1 = code[ip + 1];
    let w2 = code[ip + 2];
    let dest = (w1 >> 8) as usize;
    let callee_reg = (w1 & 0xFF) as usize;
    let total = (w2 >> 8) as usize;
    let arg_start = (w2 & 0xFF) as usize;

    let callee = box_or_pass(b, actx.vars, state, callee_reg);

    let direct = target.filter(|t| {
        t.raw_slot != 0
            && t.param_kinds.len() + 1 == total
            && arg_start + total <= actx.nregs
            && t.param_kinds.iter().enumerate().all(|(i, k)| {
                let r = arg_start + 1 + i;
                if *k != SlotKind::Int {
                    return true;
                }
                let unboxable = state[r] == K::Int || super::super::kinds::is_boxed_kind(state[r]);
                unboxable && !meta_is_float(actx.register_meta, r)
            })
    });

    let Some(t) = direct else {
        let res = emit_vm_call(b, actx, state, callee, arg_start, total);
        def_result(b, actx, dest, res);
        return Ok(());
    };

    let mut raw_args = Vec::with_capacity(1 + t.param_kinds.len());
    raw_args.push(actx.exec_ctx);
    for (i, k) in t.param_kinds.iter().enumerate() {
        let r = arg_start + 1 + i;
        let v = if *k == SlotKind::Int {
            use_int(b, actx.vars, state, r)?
        } else if *k == SlotKind::Float {
            if meta_is_float(actx.register_meta, r) || state[r] == K::Float {
                let f = use_f64(b, actx.vars, state, r)?;
                b.ins().bitcast(types::I64, MemFlags::trusted(), f)
            } else {
                let boxed = box_or_pass(b, actx.vars, state, r);
                let f = unbox_f64_coerce(b, boxed);
                b.ins().bitcast(types::I64, MemFlags::trusted(), f)
            }
        } else if *k == SlotKind::Bool {
            let boxed = box_or_pass(b, actx.vars, state, r);
            unbox_bool(b, boxed)
        } else {
            box_or_pass(b, actx.vars, state, r)
        };
        raw_args.push(v);
    }

    let expected = b.ins().iconst(types::I64, t.expected_bits as i64);
    let same = b.ins().icmp(IntCC::Equal, callee, expected);
    let slot = b.ins().iconst(types::I64, t.raw_slot as i64);
    let raw = b.ins().load(types::I64, MemFlags::trusted(), slot, 0);
    let published = b.ins().icmp_imm(IntCC::NotEqual, raw, 0);
    let take_direct = b.ins().band(same, published);

    let fast = b.create_block();
    let slow = b.create_block();
    let merge = b.create_block();
    b.append_block_param(merge, types::I64);
    b.ins().brif(take_direct, fast, &[], slow, &[]);

    b.switch_to_block(fast);
    let raw_sig = {
        let mut s = cranelift_codegen::ir::Signature::new(actx.cc);
        for _ in 0..raw_args.len() {
            s.params
                .push(cranelift_codegen::ir::AbiParam::new(types::I64));
        }
        s.returns
            .push(cranelift_codegen::ir::AbiParam::new(types::I64));
        b.import_signature(s)
    };
    let call = b.ins().call_indirect(raw_sig, raw, &raw_args);
    let raw_res = b.inst_results(call)[0];
    let boxed_fast = super::super::emit::retag_raw_return(b, raw_res, t.return_kind);
    b.ins().jump(merge, &[boxed_fast.into()]);

    b.switch_to_block(slow);
    let boxed_slow = emit_vm_call(b, actx, state, callee, arg_start, total);
    b.ins().jump(merge, &[boxed_slow.into()]);

    b.switch_to_block(merge);
    let res = b.block_params(merge)[0];
    def_result(b, actx, dest, res);
    Ok(())
}

fn emit_vm_call(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    callee: cranelift_codegen::ir::Value,
    arg_start: usize,
    total: usize,
) -> cranelift_codegen::ir::Value {
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let fb = frame_base_addr(b, actx);
    for r in arg_start..(arg_start + total).min(actx.nregs) {
        store_home(b, actx, state, fb, r);
    }

    let src = b.ins().iadd_imm(actx.base, arg_start as i64);
    let n = b.ins().iconst(types::I64, total as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.clif_call_fallback,
        &[actx.exec_ctx, callee, src, n],
    );
    reload_boxed(b, actx, state, &regs);
    res
}

pub(crate) fn emit_get_property(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    _meta: &[varn_types::register_meta::RegisterMeta],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let obj_r = (code[ip + 1] >> 8) as usize;
    let cs_idx = (code[ip + 1] & 0xFF) as usize;
    let name_idx = code[ip + 2] as usize;
    let next_ip = ip + 3;

    let obj = box_or_pass(b, actx.vars, state, obj_r);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let ni = b.ins().iconst(types::I64, name_idx as i64);
    let ci = b.ins().iconst(types::I64, cs_idx as i64);
    let de = b.ins().iconst(types::I64, dest as i64);
    let ipv = b.ins().iconst(types::I64, next_ip as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.get_property_flat,
        &[actx.exec_ctx, actx.closure, actx.base, obj, ni, ci, de, ipv],
    );

    reload_boxed(b, actx, state, &regs);

    def_result(b, actx, dest, res);
}

pub(crate) fn emit_set_property(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let obj_r = (code[ip] >> 8) as usize;
    let val_r = (code[ip + 1] >> 8) as usize;
    let cs_idx = (code[ip + 1] & 0xFF) as usize;
    let name_idx = code[ip + 2] as usize;
    let next_ip = ip + 3;

    let obj = box_or_pass(b, actx.vars, state, obj_r);
    let val = box_or_pass(b, actx.vars, state, val_r);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let ni = b.ins().iconst(types::I64, name_idx as i64);
    let ci = b.ins().iconst(types::I64, cs_idx as i64);
    let ipv = b.ins().iconst(types::I64, next_ip as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.set_property_flat,
        &[actx.exec_ctx, actx.closure, obj, val, ni, ci, ipv],
    );

    reload_boxed(b, actx, state, &regs);
}
