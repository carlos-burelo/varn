use cranelift_codegen::ir::{types, InstBuilder};
use cranelift_frontend::FunctionBuilder;

use super::super::emit::{box_or_pass, call_helper, call_helper_void};
use super::super::kinds::K;
use super::safepoints::{
    args_struct, def_result, flush_boxed, frame_base_addr, live_boxed, reload_boxed, store_home,
    AllocCtx,
};

pub(crate) fn emit_build_array(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let w1 = code[ip + 1];
    let w2 = code[ip + 2];
    let dest = (w1 >> 8) as usize;
    let start = (w1 & 0xFF) as usize;
    let count = (w2 >> 8) as usize;

    let fb = frame_base_addr(b, actx);
    for i in 0..count {
        store_home(b, actx, state, fb, start + i);
    }
    let start_v = b.ins().iconst(types::I64, start as i64);
    let count_v = b.ins().iconst(types::I64, count as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.build_array,
        &[actx.exec_ctx, actx.base, start_v, count_v],
    );
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_array_push(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let arr_r = (code[ip] >> 8) as usize;
    let val_r = (code[ip + 1] >> 8) as usize;
    let arr = box_or_pass(b, actx.vars, state, arr_r);
    let val = box_or_pass(b, actx.vars, state, val_r);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    call_helper_void(
        b,
        actx.cc,
        actx.helpers.array_push,
        &[actx.exec_ctx, arr, val],
    );

    reload_boxed(b, actx, state, &regs);
}

pub(crate) fn emit_array_extend(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let arr_r = (code[ip] >> 8) as usize;
    let src_r = (code[ip + 1] >> 8) as usize;
    let arr = box_or_pass(b, actx.vars, state, arr_r);
    let src = box_or_pass(b, actx.vars, state, src_r);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.array_extend,
        &[actx.exec_ctx, arr, src],
    );
    reload_boxed(b, actx, state, &regs);
}

pub(crate) fn emit_get_index(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let obj_r = (code[ip + 1] >> 8) as usize;
    let idx_r = (code[ip + 1] & 0xFF) as usize;
    let obj = box_or_pass(b, actx.vars, state, obj_r);
    let idx = box_or_pass(b, actx.vars, state, idx_r);
    let dest_v = b.ins().iconst(types::I64, dest as i64);
    let args = args_struct(b, &[obj, idx, dest_v]);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let res = call_helper(b, actx.cc, actx.helpers.get_index, &[actx.exec_ctx, args]);
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_set_index(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let obj_r = (code[ip] >> 8) as usize;
    let idx_r = (code[ip + 1] >> 8) as usize;
    let val_r = (code[ip + 1] & 0xFF) as usize;
    let obj = box_or_pass(b, actx.vars, state, obj_r);
    let idx = box_or_pass(b, actx.vars, state, idx_r);
    let val = box_or_pass(b, actx.vars, state, val_r);
    let args = args_struct(b, &[obj, idx, val]);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    call_helper_void(b, actx.cc, actx.helpers.set_index, &[actx.exec_ctx, args]);
    reload_boxed(b, actx, state, &regs);
}

pub(crate) fn emit_wrap_spread(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;
    let val = box_or_pass(b, actx.vars, state, src);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let res = call_helper(b, actx.cc, actx.helpers.wrap_spread, &[actx.exec_ctx, val]);
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}
