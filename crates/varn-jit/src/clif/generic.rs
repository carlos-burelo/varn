//! Generic (helper-dispatched) binary ops for CLIF: arithmetic on values not
//! statically proven int (`Add`/`Sub`/`Mul`/`Div`/`Mod`) and generic
//! comparisons (`Eq`/`Neq`/`Lt`/`Gt`/`Lte`/`Gte`). These lower to a runtime
//! helper on boxed operands — they don't need the frame-aware machinery
//! (no home slots, no safepoint around them: a helper allocates at most, and
//! allocation never triggers a collection), so they work in ANY routed
//! function. Comparison results are unboxed to 0/1 so branches read them
//! directly; arithmetic results stay boxed (unboxed at an int use).

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_core::OpCode;
use varn_types::VmValue;

use super::emit::{box_or_pass, call_helper, meta_is_float, unbox_bool, unbox_f64_coerce};
use super::kinds::K;
use crate::JitHelpers;

/// General lowering context (not frame-specific). Helper addresses are passed
/// per-op, so this only carries the operand/dispatch essentials.
pub(super) struct GenCtx<'a> {
    pub vars: &'a [Variable],
    pub cc: CallConv,
    pub exec_ctx: cranelift_codegen::ir::Value,
    pub register_meta: &'a [varn_types::register_meta::RegisterMeta],
}

/// Define `dest` from a helper's boxed `VmValue` result, coercing when the
/// register is float-typed — its Variable is `F64`, so the raw bits would be
/// a Cranelift type error. Matches the kind `apply_kinds` assigns.
fn def_boxed(b: &mut FunctionBuilder, g: &GenCtx, dest: usize, res: cranelift_codegen::ir::Value) {
    if meta_is_float(g.register_meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(g.vars[dest], f);
    } else {
        b.def_var(g.vars[dest], res);
    }
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
    def_boxed(b, g, dest, res);
}

/// `IsNull dest, src` — compare against the null VmValue bits (0/1 result).
pub(super) fn emit_is_null(
    b: &mut FunctionBuilder,
    g: &GenCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;
    let is_null = match state[src] {
        K::Int | K::Float | K::Bool => b.ins().iconst(types::I64, 0),
        _ => {
            let v = box_or_pass(b, g.vars, state, src);
            let cmp = b.ins().icmp_imm(IntCC::Equal, v, VmValue::null().0 as i64);
            b.ins().uextend(types::I64, cmp)
        }
    };
    b.def_var(g.vars[dest], is_null);
}

/// Dispatch a generic (helper-based) op. Returns `true` if `op` was one of
/// them and has been emitted, `false` otherwise (the caller then bails).
/// Keeps the whole family — arithmetic, comparisons, unary — in one place.
pub(super) fn try_emit(
    b: &mut FunctionBuilder,
    g: &GenCtx,
    h: &JitHelpers,
    state: &[K],
    op: OpCode,
    code: &[u16],
    ip: usize,
) -> bool {
    // A raw 0/1 result cannot live in a float-typed register (its Variable is
    // `F64`), and a bool has no meaningful float representation — so a
    // comparison landing in one bails the whole function to the interpreter
    // rather than being coerced into nonsense.
    let bool_result = matches!(
        op,
        OpCode::Eq
            | OpCode::Neq
            | OpCode::Lt
            | OpCode::Gt
            | OpCode::Lte
            | OpCode::Gte
            | OpCode::EqFloat
            | OpCode::NeqFloat
            | OpCode::LtFloat
            | OpCode::GtFloat
            | OpCode::LteFloat
            | OpCode::GteFloat
            | OpCode::Instanceof
            | OpCode::In
            | OpCode::Not
            | OpCode::IsArray
            | OpCode::IsNull
    );
    if bool_result && meta_is_float(g.register_meta, (code[ip] >> 8) as usize) {
        return false;
    }
    match op {
        // Numeric/string/float arithmetic — the runtime helpers dispatch by
        // operand type, so the typed *Float variants share them (operands are
        // boxed float VmValues here; native f64 is a future perf lever).
        OpCode::Add | OpCode::AddFloat => emit_binop(b, g, state, code, ip, h.add),
        OpCode::Sub | OpCode::SubFloat => emit_binop(b, g, state, code, ip, h.sub),
        OpCode::Mul | OpCode::MulFloat => emit_binop(b, g, state, code, ip, h.mul),
        OpCode::Div | OpCode::DivFloat | OpCode::DivInt => emit_binop(b, g, state, code, ip, h.div),
        OpCode::Mod | OpCode::ModFloat => emit_binop(b, g, state, code, ip, h.modulo),
        OpCode::Pow | OpCode::PowInt | OpCode::PowFloat => emit_binop(b, g, state, code, ip, h.pow),
        OpCode::Eq | OpCode::EqFloat => emit_compare(b, g, state, code, ip, h.eq),
        OpCode::Neq | OpCode::NeqFloat => emit_compare(b, g, state, code, ip, h.neq),
        OpCode::Lt | OpCode::LtFloat => emit_compare(b, g, state, code, ip, h.lt),
        OpCode::Gt | OpCode::GtFloat => emit_compare(b, g, state, code, ip, h.gt),
        OpCode::Lte | OpCode::LteFloat => emit_compare(b, g, state, code, ip, h.lte),
        OpCode::Gte | OpCode::GteFloat => emit_compare(b, g, state, code, ip, h.gte),
        OpCode::Instanceof => emit_compare(b, g, state, code, ip, h.instanceof),
        OpCode::In => emit_compare(b, g, state, code, ip, h.op_in),
        // NB: DivInt is deliberately NOT routed. It is int/int -> float, but
        // stdlib code (`int_div`) relies on the interpreter coercing the
        // whole-number float back to an int at an `int` return; clif's typed
        // int-return unbox would misread the float bits. Let it bail.
        OpCode::BitAnd => emit_binop(b, g, state, code, ip, h.bit_and),
        OpCode::BitOr => emit_binop(b, g, state, code, ip, h.bit_or),
        OpCode::BitXor => emit_binop(b, g, state, code, ip, h.bit_xor),
        OpCode::Shl => emit_binop(b, g, state, code, ip, h.shl),
        OpCode::Shr => emit_binop(b, g, state, code, ip, h.shr),
        OpCode::Ushr => emit_binop(b, g, state, code, ip, h.ushr),
        OpCode::StrSlice => emit_binop(b, g, state, code, ip, h.str_slice),
        OpCode::StrLength => emit_unary(b, g, state, code, ip, h.str_length),
        OpCode::ArrayPop => emit_unary(b, g, state, code, ip, h.array_pop),
        OpCode::Negate => emit_unary(b, g, state, code, ip, h.negate),
        OpCode::Typeof => emit_unary(b, g, state, code, ip, h.typeof_val),
        OpCode::ToString => emit_unary(b, g, state, code, ip, h.to_string),
        OpCode::Not => emit_unary_bool(b, g, state, code, ip, h.logical_not),
        OpCode::IsArray => emit_unary_bool(b, g, state, code, ip, h.is_array),
        OpCode::GetSymbol => emit_get_symbol(b, g, state, code, ip, h.get_symbol),
        OpCode::IsNull => emit_is_null(b, g, state, code, ip),
        _ => return false,
    }
    true
}

/// `dest, src` generic unary via `helper(ctx, v) -> VmValue` (Negate,
/// Typeof, ToString). Result is boxed bits.
pub(super) fn emit_unary(
    b: &mut FunctionBuilder,
    g: &GenCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    helper: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;
    let v = box_or_pass(b, g.vars, state, src);
    let res = call_helper(b, g.cc, helper, &[g.exec_ctx, v]);
    def_boxed(b, g, dest, res);
}

/// `dest, src` unary producing a boxed bool (`Not` = logical_not); unboxed to
/// 0/1 (`K::Bool`).
pub(super) fn emit_unary_bool(
    b: &mut FunctionBuilder,
    g: &GenCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    helper: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;
    let v = box_or_pass(b, g.vars, state, src);
    let res = call_helper(b, g.cc, helper, &[g.exec_ctx, v]);
    let cond = unbox_bool(b, res);
    b.def_var(g.vars[dest], cond);
}

/// `GetSymbol dest, obj, sym_idx` — well-known symbol property access;
/// `jit_get_symbol` reads the symbol from the current frame's closure.
pub(super) fn emit_get_symbol(
    b: &mut FunctionBuilder,
    g: &GenCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    get_symbol: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let obj_r = (code[ip + 1] >> 8) as usize;
    let sym_idx = code[ip + 2] as usize;
    let obj = box_or_pass(b, g.vars, state, obj_r);
    let sym = b.ins().iconst(types::I64, sym_idx as i64);
    let res = call_helper(b, g.cc, get_symbol, &[g.exec_ctx, obj, sym]);
    def_boxed(b, g, dest, res);
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
