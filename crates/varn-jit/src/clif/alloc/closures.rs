use cranelift_codegen::ir::{types, InstBuilder};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::{RegisterMeta, SlotKind};

use super::super::emit::{box_or_pass, call_helper, call_helper_void};
use super::super::kinds::K;
use super::safepoints::{
    def_result, flush_boxed, frame_base_addr, live_boxed, reload_boxed, store_home, AllocCtx,
};

pub(crate) fn emit_load_upvalue(b: &mut FunctionBuilder, actx: &AllocCtx, code: &[u16], ip: usize) {
    let dest = (code[ip + 1] >> 8) as usize;
    let uv_idx = (code[ip + 1] & 0xFF) as usize;
    let uv_v = b.ins().iconst(types::I64, uv_idx as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.load_upvalue,
        &[actx.exec_ctx, actx.closure, uv_v],
    );
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_store_upvalue(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let uv_idx = (code[ip + 1] >> 8) as usize;
    let src_r = (code[ip + 1] & 0xFF) as usize;
    let uv_v = b.ins().iconst(types::I64, uv_idx as i64);
    let val = box_or_pass(b, actx.vars, state, src_r);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.store_upvalue,
        &[actx.exec_ctx, actx.closure, uv_v, val],
    );
}

pub(crate) fn emit_close_upvalue(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let reg = (code[ip + 1] >> 8) as usize;
    let fb = frame_base_addr(b, actx);
    store_home(b, actx, state, fb, reg);
    let reg_v = b.ins().iconst(types::I64, reg as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.close_upvalue,
        &[actx.exec_ctx, reg_v],
    );
}

pub(crate) fn emit_make_closure(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip + 1] >> 8) as usize;
    let uv_count = (code[ip + 1] & 0xFF) as usize;
    let fb = frame_base_addr(b, actx);
    for i in 0..uv_count {
        let uv_desc = code[ip + 3 + i];
        let is_local = (uv_desc >> 8) != 0;
        let index = (uv_desc & 0xFF) as usize;
        if is_local {
            store_home(b, actx, state, fb, index);
        }
    }
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let ip_v = b.ins().iconst(types::I64, ip as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.make_closure,
        &[actx.exec_ctx, actx.closure, ip_v, actx.base],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_load_static_fn(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let proto_idx = code[ip + 1] as usize;
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let idx_v = b.ins().iconst(types::I64, proto_idx as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.load_static_fn,
        &[actx.exec_ctx, actx.closure, idx_v],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_call_spread(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    meta: &[RegisterMeta],
    code: &[u16],
    ip: usize,
) {
    let w1 = code[ip + 1];
    let w2 = code[ip + 2];
    let dest = (w1 >> 8) as usize;
    let callee_reg = (w1 & 0xFF) as usize;
    let argc = (w2 >> 8) as usize;
    let arg_start = (w2 & 0xFF) as usize;

    let callee = box_or_pass(b, actx.vars, state, callee_reg);
    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        40,
        0,
    ));

    let arg_start_v = b.ins().iconst(types::I64, arg_start as i64);
    let argc_v = b.ins().iconst(types::I64, argc as i64);
    let dest_v = b.ins().iconst(types::I64, dest as i64);
    let next_ip_v = b.ins().iconst(types::I64, (ip + 3) as i64);

    b.ins().stack_store(callee, slot, 0);
    b.ins().stack_store(arg_start_v, slot, 8);
    b.ins().stack_store(argc_v, slot, 16);
    b.ins().stack_store(dest_v, slot, 24);
    b.ins().stack_store(next_ip_v, slot, 32);

    let slot_addr = b.ins().stack_addr(types::I64, slot, 0);

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let fb = frame_base_addr(b, actx);
    for r in arg_start..(arg_start + argc).min(actx.nregs) {
        store_home(b, actx, state, fb, r);
    }

    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.call_spread,
        &[actx.exec_ctx, slot_addr],
    );

    reload_boxed(b, actx, state, &regs);

    if meta.get(dest).map_or(false, |m| m.kind == SlotKind::Int) {
        let s = b.ins().ishl_imm(res, 16);
        let un = b.ins().sshr_imm(s, 16);
        b.def_var(actx.vars[dest], un);
    } else {
        b.def_var(actx.vars[dest], res);
    }
}
