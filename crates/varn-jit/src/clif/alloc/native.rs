use cranelift_codegen::ir::{types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use varn_types::chunk::{Literal, PoolEntry};
use varn_types::register_meta::RegisterMeta;

use super::super::emit::{call_helper, call_helper_void, meta_is_float, unbox_f64_coerce};
use super::super::kinds::K;
use super::safepoints::{
    box_or_load_home, def_result, flush_boxed, frame_base_addr, live_boxed, reload_boxed, store_home,
    AllocCtx,
};

pub(crate) fn emit_call_native_op(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    _meta: &[RegisterMeta],
    code: &[u16],
    pool: &[PoolEntry],
    ip: usize,
) -> Result<(), String> {
    let dest = (code[ip] >> 8) as usize;
    let cidx = code[ip + 1] as usize;
    let total = code[ip + 2] as usize;
    let op_id = match pool.get(cidx) {
        Some(PoolEntry::Literal(Literal::Int(i))) => *i as u64,
        _ => return Err("clif: native op-id not an int constant".into()),
    };

    let fb = frame_base_addr(b, actx);
    for r in dest..(dest + total).min(actx.nregs) {
        store_home(b, actx, state, fb, r);
    }
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let args_start = b.ins().iadd_imm(actx.base, dest as i64);
    let total_v = b.ins().iconst(types::I64, total as i64);
    let target = (actx.helpers.resolve_native_op)(op_id);
    let fn_addr = target.func_ptr;

    if fn_addr != 0 {
        let fn_v = b.ins().iconst(types::I64, fn_addr as i64);
        call_helper_void(
            b,
            actx.cc,
            actx.helpers.jit_call_native_fnptr,
            &[actx.exec_ctx, fn_v, args_start, total_v],
        );
    } else {
        let op_id_v = b.ins().iconst(types::I64, op_id as i64);
        call_helper_void(
            b,
            actx.cc,
            actx.helpers.jit_call_native_op,
            &[actx.exec_ctx, op_id_v, args_start, total_v],
        );
    }
    reload_boxed(b, actx, state, &regs);
    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
    );
    def_result(b, actx, dest, res);
    Ok(())
}

pub(crate) fn emit_make_enum_variant(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip + 1] >> 8) as usize;
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let tag_reg = (code[ip + 1] & 0xFF) as usize;
    let fb = frame_base_addr(b, actx);
    store_home(b, actx, state, fb, tag_reg);

    let ip_v = b.ins().iconst(types::I64, (ip + 1) as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.make_enum_variant,
        &[actx.exec_ctx, ip_v],
    );

    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_get_enum_tag(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    meta: &[RegisterMeta],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;

    let val = box_or_load_home(b, actx, state, src);
    let (val_tag, val_payload) = b.ins().isplit(val);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    call_helper_void(
        b,
        actx.cc,
        actx.helpers.get_enum_tag,
        &[actx.exec_ctx, val_tag, val_payload],
    );

    reload_boxed(b, actx, state, &regs);

    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
    );

    if meta_is_float(meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(actx.vars[dest], f);
    } else {
        let payload = if b.func.dfg.value_type(res) == cranelift_codegen::ir::types::I128 {
            let (_tag, payload) = b.ins().isplit(res);
            payload
        } else {
            res
        };
        b.def_var(actx.vars[dest], payload);
    }
}

pub(crate) fn emit_build_str(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let count = (code[ip + 1] >> 8) as usize;

    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        (count * 16) as u32,
        4,
    ));
    let parts_ptr = b.ins().stack_addr(types::I64, slot, 0);

    let vals: Vec<_> = (0..count)
        .map(|i| {
            let r = (code[ip + 2 + i] >> 8) as usize;
            box_or_load_home(b, actx, state, r)
        })
        .collect();

    for (i, val) in vals.into_iter().enumerate() {
        b.ins()
            .store(MemFlags::trusted(), val, parts_ptr, (i * 16) as i32);
    }

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let count_v = b.ins().iconst(types::I64, count as i64);

    call_helper_void(
        b,
        actx.cc,
        actx.helpers.build_str,
        &[actx.exec_ctx, parts_ptr, count_v],
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

pub(crate) fn emit_intrinsic(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    _meta: &[RegisterMeta],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let w1 = code[ip + 1];
    let wire_byte = (w1 >> 8) as usize;
    let arg_count = (w1 & 0xFF) as usize;

    let fb = frame_base_addr(b, actx);
    for r in dest..dest + arg_count {
        store_home(b, actx, state, fb, r);
    }

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let wire_v = b.ins().iconst(types::I64, wire_byte as i64);
    let start_v = b.ins().iadd_imm(actx.base, dest as i64);
    let count_v = b.ins().iconst(types::I64, arg_count as i64);

    call_helper_void(
        b,
        actx.cc,
        actx.helpers.dispatch_intrinsic,
        &[actx.exec_ctx, wire_v, start_v, count_v],
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

pub(crate) fn emit_to_string(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    _meta: &[RegisterMeta],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;

    let v = box_or_load_home(b, actx, state, src);
    let (v_tag, v_payload) = b.ins().isplit(v);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    call_helper_void(
        b,
        actx.cc,
        actx.helpers.to_string,
        &[actx.exec_ctx, v_tag, v_payload],
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

pub(crate) fn emit_invoke_runtime_static(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) -> Result<(), String> {
    let dest = (code[ip + 1] >> 8) as usize;
    let start_reg = (code[ip + 3] & 0xFF) as usize;
    let end_reg = (code[ip + 4] >> 8) as usize;
    let flag = (code[ip + 4] & 0xFF) as usize;

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let start_v = box_or_load_home(b, actx, state, start_reg);
    let end_v = box_or_load_home(b, actx, state, end_reg);
    let (start_tag, start_payload) = b.ins().isplit(start_v);
    let (end_tag, end_payload) = b.ins().isplit(end_v);
    let flag_v = b.ins().iconst(types::I64, flag as i64);

    call_helper_void(
        b,
        actx.cc,
        actx.helpers.range,
        &[
            actx.exec_ctx,
            start_tag,
            start_payload,
            end_tag,
            end_payload,
            flag_v,
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
    Ok(())
}
