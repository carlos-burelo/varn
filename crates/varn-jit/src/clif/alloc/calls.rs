//! Cross-function calls, property access, and method invocations for CLIF allocation lowering.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;

use super::super::emit::{
    call_helper_void, meta_is_float, unbox_bool, unbox_f64_coerce, use_f64, use_int,
};
use super::super::kinds::K;
use super::safepoints::{
    box_or_load_home, def_result, flush_boxed, frame_base_addr, live_boxed, reload_boxed,
    store_home, AllocCtx,
};

pub(crate) fn emit_call(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    target: Option<&crate::clif::lower::ClifTarget>,
    class_target: Option<&crate::clif::lower::ClifClassTarget>,
) -> Result<(), String> {
    let w1 = code[ip + 1];
    let w2 = code[ip + 2];
    let dest = (w1 >> 8) as usize;
    let callee_reg = (w1 & 0xFF) as usize;
    let total = (w2 >> 8) as usize;
    let arg_start = (w2 & 0xFF) as usize;

    let callee = box_or_load_home(b, actx, state, callee_reg);

    if let Some(ct) = class_target {
        if let Some(ref plan) = ct.trivial_plan {
            let valid_plan = plan.iter().all(|&(param_idx, _)| {
                1 + param_idx < total && arg_start + 1 + param_idx < actx.nregs
            });
            if valid_plan && arg_start + total <= actx.nregs {
                let fast_blk = b.create_block();
                let slow_blk = b.create_block();
                let cont_blk = b.create_block();

                let (callee_tag, callee_payload) = b.ins().isplit(callee);
                let expected_tag =
                    b.ins().iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64);
                let expected_payload = b.ins().iconst(types::I64, ct.expected_bits as i64);
                let tag_matches = b.ins().icmp(IntCC::Equal, callee_tag, expected_tag);
                let payload_matches = b.ins().icmp(IntCC::Equal, callee_payload, expected_payload);
                let callee_ok = b.ins().band(tag_matches, payload_matches);
                b.ins().brif(callee_ok, fast_blk, &[], slow_blk, &[]);

                b.switch_to_block(fast_blk);
                let cid_val = b.ins().iconst(types::I64, ct.class_id as i64);
                let raw_idx = super::super::emit::call_helper(
                    b,
                    actx.cc,
                    actx.helpers.alloc_instance_fast,
                    &[actx.exec_ctx, cid_val],
                );
                let inst_tag = b
                    .ins()
                    .iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64);
                let instance_nv = b.ins().iconcat(inst_tag, raw_idx);

                let data_base = super::super::emit::emit_object_data_base(
                    b,
                    actx.exec_ctx,
                    instance_nv,
                    &actx.helpers.object_layout,
                    &actx.helpers.array_layout,
                    actx.helpers.heap_field_offset,
                    slow_blk,
                );

                for &(param_idx, slot) in plan {
                    let arg_r = arg_start + 1 + param_idx;
                    let val = box_or_load_home(b, actx, state, arg_r);
                    let val128 = if b.func.dfg.value_type(val) == types::I128 {
                        val
                    } else if b.func.dfg.value_type(val) == types::F64 {
                        super::super::emit::box_f64(b, val)
                    } else {
                        match state.get(arg_r).copied().unwrap_or(K::Unset) {
                            K::Int => super::super::emit::box_int(b, val),
                            K::Bool => super::super::emit::box_bool(b, val),
                            K::Float => {
                                let f = b.ins().bitcast(types::F64, MemFlags::new(), val);
                                super::super::emit::box_f64(b, f)
                            }
                            _ => {
                                let tag_v = b.ins().iconst(
                                    types::I64,
                                    varn_types::vm_value::KIND_HEAP as i64,
                                );
                                b.ins().iconcat(tag_v, val)
                            }
                        }
                    };
                    let slot_off = (slot * 16) as i32;
                    b.ins().store(MemFlags::trusted(), val128, data_base, slot_off);
                }

                def_result(b, actx, dest, instance_nv);
                b.ins().jump(cont_blk, &[]);

                b.switch_to_block(slow_blk);
                let slow_res = emit_vm_call(b, actx, state, callee, arg_start, total);
                def_result(b, actx, dest, slow_res);
                b.ins().jump(cont_blk, &[]);

                b.switch_to_block(cont_blk);
                return Ok(());
            }
        }
    }

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
                use_f64(b, actx.vars, state, r)?
            } else {
                let boxed = box_or_load_home(b, actx, state, r);
                unbox_f64_coerce(b, boxed)
            }
        } else if *k == SlotKind::Bool {
            let boxed = box_or_load_home(b, actx, state, r);
            unbox_bool(b, boxed)
        } else {
            let boxed = box_or_load_home(b, actx, state, r);
            let (_tag, payload) = b.ins().isplit(boxed);
            payload
        };
        raw_args.push(v);
    }

    let (callee_tag, callee_payload) = b.ins().isplit(callee);
    let expected_tag = b
        .ins()
        .iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64);
    let same_tag = b.ins().icmp(IntCC::Equal, callee_tag, expected_tag);
    let expected_payload = b.ins().iconst(types::I64, t.expected_bits as i64);
    let same_payload = b.ins().icmp(IntCC::Equal, callee_payload, expected_payload);
    let same = b.ins().band(same_tag, same_payload);
    let slot = b.ins().iconst(types::I64, t.raw_slot as i64);
    let raw = b.ins().load(types::I64, MemFlags::trusted(), slot, 0);
    let published = b.ins().icmp_imm(IntCC::NotEqual, raw, 0);
    let take_direct = b.ins().band(same, published);

    let fast = b.create_block();
    let slow = b.create_block();
    let merge = b.create_block();
    b.append_block_param(merge, types::I128);
    b.ins().brif(take_direct, fast, &[], slow, &[]);

    b.switch_to_block(fast);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let raw_sig = {
        let mut s = cranelift_codegen::ir::Signature::new(actx.cc);
        s.params
            .push(cranelift_codegen::ir::AbiParam::new(types::I64));
        for k in &t.param_kinds {
            if *k == SlotKind::Float {
                s.params
                    .push(cranelift_codegen::ir::AbiParam::new(types::F64));
            } else {
                s.params
                    .push(cranelift_codegen::ir::AbiParam::new(types::I64));
            }
        }
        if t.return_kind == SlotKind::Int || t.return_kind == SlotKind::Bool {
            s.returns
                .push(cranelift_codegen::ir::AbiParam::new(types::I64));
        } else if t.return_kind == SlotKind::Float {
            s.returns
                .push(cranelift_codegen::ir::AbiParam::new(types::F64));
        }
        b.import_signature(s)
    };
    let boxed_fast = if t.return_kind == SlotKind::Int
        || t.return_kind == SlotKind::Bool
        || t.return_kind == SlotKind::Float
    {
        let call = b.ins().call_indirect(raw_sig, raw, &raw_args);
        let raw_res = b.inst_results(call)[0];
        super::super::emit::retag_raw_return(b, raw_res, t.return_kind)
    } else {
        b.ins().call_indirect(raw_sig, raw, &raw_args);
        b.ins().load(
            types::I128,
            MemFlags::trusted(),
            actx.exec_ctx,
            actx.helpers.jit_native_result_offset as i32,
        )
    };
    reload_boxed(b, actx, state, &regs);
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

    let (callee_tag, callee_payload) = b.ins().isplit(callee);
    let src = b.ins().iadd_imm(actx.base, arg_start as i64);
    let n = b.ins().iconst(types::I64, total as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.clif_call_fallback,
        &[actx.exec_ctx, callee_tag, callee_payload, src, n],
    );
    reload_boxed(b, actx, state, &regs);
    b.ins().load(
        types::I128,
        MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
    )
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

    let obj = box_or_load_home(b, actx, state, obj_r);
    let (obj_tag, obj_payload) = b.ins().isplit(obj);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let ni = b.ins().iconst(types::I64, name_idx as i64);
    let ci = b.ins().iconst(types::I64, cs_idx as i64);
    let de = b.ins().iconst(types::I64, dest as i64);
    let ipv = b.ins().iconst(types::I64, next_ip as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.get_property_flat,
        &[
            actx.exec_ctx,
            actx.closure,
            actx.base,
            obj_tag,
            obj_payload,
            ni,
            ci,
            de,
            ipv,
        ],
    );

    reload_boxed(b, actx, state, &regs);

    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
    );
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

    let obj = box_or_load_home(b, actx, state, obj_r);
    let val = box_or_load_home(b, actx, state, val_r);
    let (obj_tag, obj_payload) = b.ins().isplit(obj);
    let (val_tag, val_payload) = b.ins().isplit(val);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let ni = b.ins().iconst(types::I64, name_idx as i64);
    let ci = b.ins().iconst(types::I64, cs_idx as i64);
    let ipv = b.ins().iconst(types::I64, next_ip as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.set_property_flat,
        &[
            actx.exec_ctx,
            actx.closure,
            obj_tag,
            obj_payload,
            val_tag,
            val_payload,
            ni,
            ci,
            ipv,
        ],
    );

    reload_boxed(b, actx, state, &regs);
}
