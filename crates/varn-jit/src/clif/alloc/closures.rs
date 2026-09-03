use cranelift_codegen::ir::{types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::RegisterMeta;

use super::super::emit::call_helper_void;
use super::super::kinds::K;
use super::safepoints::{
    box_or_load_home, def_result, flush_boxed, frame_base_addr, live_boxed, reload_boxed, store_home,
    AllocCtx,
};

pub(crate) fn emit_load_upvalue(b: &mut FunctionBuilder, actx: &AllocCtx, code: &[u16], ip: usize) {
    let dest = (code[ip + 1] >> 8) as usize;
    let uv_idx = (code[ip + 1] & 0xFF) as usize;
    let uv_v = b.ins().iconst(types::I64, uv_idx as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.load_upvalue,
        &[actx.exec_ctx, actx.closure, uv_v],
    );
    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
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
    let val = box_or_load_home(b, actx, state, src_r);
    let (val_tag, val_payload) = b.ins().isplit(val);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.store_upvalue,
        &[actx.exec_ctx, actx.closure, uv_v, val_tag, val_payload],
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
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.make_closure,
        &[actx.exec_ctx, actx.closure, ip_v, actx.base],
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
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.load_static_fn,
        &[actx.exec_ctx, actx.closure, idx_v],
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

pub(crate) fn emit_call_spread(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    _meta: &[RegisterMeta],
    code: &[u16],
    ip: usize,
) {
    let w1 = code[ip + 1];
    let w2 = code[ip + 2];
    let dest = (w1 >> 8) as usize;
    let callee_reg = (w1 & 0xFF) as usize;
    let argc = (w2 >> 8) as usize;
    let arg_start = (w2 & 0xFF) as usize;

    let callee = box_or_load_home(b, actx, state, callee_reg);
    let (callee_tag, callee_payload) = b.ins().isplit(callee);
    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        48,
        3,
    ));

    let arg_start_v = b.ins().iconst(types::I64, arg_start as i64);
    let argc_v = b.ins().iconst(types::I64, argc as i64);
    let dest_v = b.ins().iconst(types::I64, dest as i64);
    let next_ip_v = b.ins().iconst(types::I64, (ip + 3) as i64);

    b.ins().stack_store(callee_tag, slot, 0);
    b.ins().stack_store(callee_payload, slot, 8);
    b.ins().stack_store(arg_start_v, slot, 16);
    b.ins().stack_store(argc_v, slot, 24);
    b.ins().stack_store(dest_v, slot, 32);
    b.ins().stack_store(next_ip_v, slot, 40);

    let slot_addr = b.ins().stack_addr(types::I64, slot, 0);

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let fb = frame_base_addr(b, actx);
    for r in arg_start..(arg_start + argc).min(actx.nregs) {
        store_home(b, actx, state, fb, r);
    }

    call_helper_void(
        b,
        actx.cc,
        actx.helpers.call_spread,
        &[actx.exec_ctx, slot_addr],
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
