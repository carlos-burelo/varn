//! Generic (helper-dispatched) binary ops for CLIF: arithmetic on values not
//! statically proven int (`Add`/`Sub`/`Mul`/`Div`/`Mod`) and generic
//! comparisons (`Eq`/`Neq`/`Lt`/`Gt`/`Lte`/`Gte`). These lower to a runtime
//! helper on boxed operands — they don't need the frame-aware machinery
//! (no home slots, no safepoint around them: a helper allocates at most, and
//! allocation never triggers a collection), so they work in ANY routed
//! function. Comparison results are unboxed to 0/1 so branches read them
//! directly; arithmetic results stay boxed (unboxed at an int use).

use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};

use super::emit::{box_or_pass, call_helper, unbox_bool};
use super::kinds::K;

/// General lowering context (not frame-specific). Helper addresses are passed
/// per-op, so this only carries the operand/dispatch essentials.
pub(super) struct GenCtx<'a> {
    pub vars: &'a [Variable],
    pub cc: CallConv,
    pub exec_ctx: cranelift_codegen::ir::Value,
}

/// `dest, a, b` generic arithmetic via `helper(ctx, a, b) -> VmValue`.
/// Result is boxed bits.
pub(super) fn emit_binop(
    b: &mut FunctionBuilder,
    g: &GenCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    helper: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let a_r = (code[ip + 1] >> 8) as usize;
    let b_r = (code[ip + 1] & 0xFF) as usize;
    let a = box_or_pass(b, g.vars, state, a_r);
    let bb = box_or_pass(b, g.vars, state, b_r);
    let res = call_helper(b, g.cc, helper, &[g.exec_ctx, a, bb]);
    b.def_var(g.vars[dest], res);
}

/// `dest, a, b` generic comparison via `helper(ctx, a, b) -> boxed bool`;
/// the result is unboxed to 0/1 (`K::Bool`).
pub(super) fn emit_compare(
    b: &mut FunctionBuilder,
    g: &GenCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    helper: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let a_r = (code[ip + 1] >> 8) as usize;
    let b_r = (code[ip + 1] & 0xFF) as usize;
    let a = box_or_pass(b, g.vars, state, a_r);
    let bb = box_or_pass(b, g.vars, state, b_r);
    let res = call_helper(b, g.cc, helper, &[g.exec_ctx, a, bb]);
    let cond = unbox_bool(b, res);
    b.def_var(g.vars[dest], cond);
}
