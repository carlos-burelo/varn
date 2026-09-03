//! `StrConcat` lowering.

use cranelift_codegen::ir::{types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;

use super::alloc::{box_or_load_home, def_result, flush_boxed, live_boxed, reload_boxed, AllocCtx};
use super::emit::call_helper_void;
use super::kinds::K;

pub(super) fn emit_str_concat(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let a_r = (code[ip + 1] >> 8) as usize;
    let b_r = (code[ip + 1] & 0xFF) as usize;
    let a = box_or_load_home(b, actx, state, a_r);
    let bb = box_or_load_home(b, actx, state, b_r);

    let (a_tag, a_payload) = b.ins().isplit(a);
    let (b_tag, b_payload) = b.ins().isplit(bb);

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.str_concat,
        &[actx.exec_ctx, a_tag, a_payload, b_tag, b_payload],
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
