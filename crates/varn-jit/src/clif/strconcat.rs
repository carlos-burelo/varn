//! `StrConcat` lowering.
//!
//! Its own module because `clif::alloc.rs` is already past this repo's
//! refactor threshold, and because the fast path this opcode grows is a
//! self-contained piece of value-representation knowledge — the SSO encoding
//! from `varn_types::vm_value` — that nothing else in `alloc` needs.

use cranelift_frontend::FunctionBuilder;

use super::alloc::{def_result, flush_boxed, live_boxed, reload_boxed, AllocCtx};
use super::emit::{box_or_pass, call_helper};
use super::kinds::K;

/// `StrConcat dest, a, b` — allocate the concatenation. Result is a boxed
/// heap string.
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
    let a = box_or_pass(b, actx.vars, state, a_r);
    let bb = box_or_pass(b, actx.vars, state, b_r);

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let res = call_helper(b, actx.cc, actx.helpers.str_concat, &[actx.exec_ctx, a, bb]);

    reload_boxed(b, actx, state, &regs);

    def_result(b, actx, dest, res);
}
