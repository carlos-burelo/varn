use cranelift_codegen::ir::{types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;

use super::super::emit::call_helper_void;
use super::super::kinds::K;
use super::safepoints::{
    box_or_load_home, def_result, flush_boxed, frame_base_addr, live_boxed, reload_boxed, store_home,
    AllocCtx,
};

pub(crate) fn emit_try_push(b: &mut FunctionBuilder, actx: &AllocCtx, code: &[u16], ip: usize) {
    let err_reg = (code[ip + 1] >> 8) as u32;
    let offset_hi = code[ip + 2] as usize;
    let offset_lo = code[ip + 3] as usize;
    let catch_offset = (offset_hi << 16) | offset_lo;
    let catch_ip = ip + 4 + catch_offset;
    let ip_v = b.ins().iconst(types::I64, catch_ip as i64);
    let err_v = b.ins().iconst(types::I64, err_reg as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.try_push,
        &[actx.exec_ctx, ip_v, err_v],
    );
}

pub(crate) fn emit_try_pop(b: &mut FunctionBuilder, actx: &AllocCtx) {
    call_helper_void(b, actx.cc, actx.helpers.try_pop, &[actx.exec_ctx]);
}

pub(crate) fn emit_throw(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let src = (code[ip + 1] >> 8) as usize;
    let val = box_or_load_home(b, actx, state, src);
    let (val_tag, val_payload) = b.ins().isplit(val);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.throw,
        &[actx.exec_ctx, val_tag, val_payload],
    );
}

pub(crate) fn emit_yield(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest_reg = (code[ip + 1] >> 8) as u32;
    let src = (code[ip + 1] & 0xFF) as usize;
    let val = box_or_load_home(b, actx, state, src);
    let (val_tag, val_payload) = b.ins().isplit(val);
    let fb = frame_base_addr(b, actx);
    for r in 0..actx.vars.len() {
        store_home(b, actx, state, fb, r);
    }
    let dest_v = b.ins().iconst(types::I64, dest_reg as i64);
    let resume_v = b.ins().iconst(types::I64, (ip + 2) as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.yield_helper,
        &[actx.exec_ctx, val_tag, val_payload, dest_v, resume_v],
    );
}

pub(crate) fn emit_await(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;
    let val = box_or_load_home(b, actx, state, src);
    let (val_tag, val_payload) = b.ins().isplit(val);
    let fb = frame_base_addr(b, actx);
    for r in 0..actx.vars.len() {
        store_home(b, actx, state, fb, r);
    }
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let dest_v = b.ins().iconst(types::I64, dest as i64);
    let resume_v = b.ins().iconst(types::I64, (ip + 2) as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.await_helper,
        &[actx.exec_ctx, val_tag, val_payload, dest_v, resume_v],
    );
    reload_boxed(b, actx, state, &regs);
}

pub(crate) fn emit_spawn(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
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
        actx.helpers.spawn,
        &[actx.exec_ctx, val_tag, val_payload],
    );
    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}
