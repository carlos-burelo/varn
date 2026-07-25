//! Native `f64` lowering for CLIF: the typed `*Float` opcode family on
//! registers proven float.
//!
//! Representation is static per register: a register `r` with
//! `register_meta[r].kind == SlotKind::Float` is an unboxed `f64` living in an
//! `F64` Cranelift Variable (declared in `lower.rs`). Every boundary crossing
//! (param entry, return, home-slot flush, generic-helper argument) boxes with
//! [`super::emit::box_f64`] — which replicates `VmValue::from_f64`'s quiet-NaN
//! canonicalization so the result is byte-identical to the interpreter — and
//! unboxes with a pure bitcast on the way back.
//!
//! `Add/Sub/Mul/DivFloat` lower to native `fadd/fsub/fmul/fdiv`; `DivFloat`
//! keeps the interpreter's divide-by-zero trap by diverting `b == 0.0` to the
//! runtime helper. `Mod/PowFloat` have no native ISA op, so they call the
//! runtime helper on boxed operands and unbox the result back into the `F64`
//! Variable. Comparisons lower to native `fcmp` and yield an unboxed `0/1`.

use cranelift_codegen::ir::{condcodes::FloatCC, types, InstBuilder, Value};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::register_meta::{RegisterMeta, SlotKind};

use super::emit::{box_f64, box_or_pass, call_helper, meta_is_float, unbox_f64, use_f64};
use super::kinds::K;
use crate::JitHelpers;

/// Whether both operands can feed a native float op (proven float, or int
/// which coerces via `fcvt_from_sint`). A boxed/untyped operand can't — it
/// falls back to the generic helper.
fn operands_native(state: &[K], a_r: usize, b_r: usize) -> bool {
    matches!(state[a_r], K::Float | K::Int) && matches!(state[b_r], K::Float | K::Int)
}

/// Emit a typed-float opcode. Returns `Ok(true)` when handled natively (or via
/// the float-boxing helper path); `Ok(false)` means the operands/dest aren't
/// float-typed and the caller should fall back to the generic helper.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_float_op(
    b: &mut FunctionBuilder,
    vars: &[Variable],
    state: &[K],
    meta: &[RegisterMeta],
    code: &[u16],
    ip: usize,
    op: OpCode,
    cc: CallConv,
    exec_ctx: Value,
    helpers: &JitHelpers,
) -> Result<bool, String> {
    let dest = (code[ip] >> 8) as usize;
    let a_r = (code[ip + 1] >> 8) as usize;
    let b_r = (code[ip + 1] & 0xFF) as usize;
    let dest_float = meta_is_float(meta, dest);

    match op {
        // A float-typed result (F64 Variable) MUST be produced here — the
        // generic helper would `def_var` an i64 into it and panic. With both
        // operands proven float/int, emit the native op; otherwise fall to the
        // runtime helper on boxed operands and unbox the result back to f64.
        OpCode::AddFloat | OpCode::SubFloat | OpCode::MulFloat | OpCode::DivFloat
            if dest_float =>
        {
            let res = if operands_native(state, a_r, b_r) {
                let a = use_f64(b, vars, state, a_r)?;
                let bb = use_f64(b, vars, state, b_r)?;
                match op {
                    OpCode::AddFloat => b.ins().fadd(a, bb),
                    OpCode::SubFloat => b.ins().fsub(a, bb),
                    OpCode::MulFloat => b.ins().fmul(a, bb),
                    OpCode::DivFloat => emit_fdiv(b, cc, exec_ctx, helpers.div, a, bb),
                    _ => unreachable!(),
                }
            } else {
                let a = box_or_pass(b, vars, state, a_r);
                let bb = box_or_pass(b, vars, state, b_r);
                let helper = match op {
                    OpCode::AddFloat => helpers.add,
                    OpCode::SubFloat => helpers.sub,
                    OpCode::MulFloat => helpers.mul,
                    _ => helpers.div,
                };
                let boxed = call_helper(b, cc, helper, &[exec_ctx, a, bb]);
                unbox_f64(b, boxed)
            };
            b.def_var(vars[dest], res);
            Ok(true)
        }
        // No native `frem`/`powf`: compute via the runtime helper on boxed
        // operands (it canonicalizes and traps on a zero modulus exactly like
        // the interpreter), then unbox the result back into the F64 Variable.
        OpCode::ModFloat | OpCode::PowFloat if dest_float => {
            let a = box_or_pass(b, vars, state, a_r);
            let bb = box_or_pass(b, vars, state, b_r);
            let helper = if op == OpCode::ModFloat {
                helpers.modulo
            } else {
                helpers.pow
            };
            let boxed = call_helper(b, cc, helper, &[exec_ctx, a, bb]);
            let f = unbox_f64(b, boxed);
            b.def_var(vars[dest], f);
            Ok(true)
        }
        OpCode::LtFloat
        | OpCode::GtFloat
        | OpCode::LteFloat
        | OpCode::GteFloat
        | OpCode::EqFloat
        | OpCode::NeqFloat
            if operands_native(state, a_r, b_r) =>
        {
            let a = use_f64(b, vars, state, a_r)?;
            let bb = use_f64(b, vars, state, b_r)?;
            // Ordered comparisons (NaN → false) match Rust's `< <= > >= ==`;
            // `NotEqual` is unordered-or-not-equal, matching Rust's `!=`.
            let fcc = match op {
                OpCode::LtFloat => FloatCC::LessThan,
                OpCode::GtFloat => FloatCC::GreaterThan,
                OpCode::LteFloat => FloatCC::LessThanOrEqual,
                OpCode::GteFloat => FloatCC::GreaterThanOrEqual,
                OpCode::EqFloat => FloatCC::Equal,
                _ => FloatCC::NotEqual,
            };
            let c = b.ins().fcmp(fcc, a, bb);
            let ext = b.ins().uextend(types::I64, c);
            b.def_var(vars[dest], ext);
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Native `fdiv` with the interpreter's divide-by-zero trap: `b == 0.0`
/// diverts to the runtime `div` helper (which raises the VM error via longjmp
/// and never returns); the common path is a plain `fdiv`. Leaves the builder
/// in the continuation block holding the `f64` quotient.
fn emit_fdiv(
    b: &mut FunctionBuilder,
    cc: CallConv,
    exec_ctx: Value,
    div_helper: usize,
    a: Value,
    bb: Value,
) -> Value {
    let zero = b.ins().f64const(0.0);
    let is_zero = b.ins().fcmp(FloatCC::Equal, bb, zero);
    let trap_blk = b.create_block();
    let div_blk = b.create_block();
    let cont = b.create_block();
    b.append_block_param(cont, types::F64);
    b.ins().brif(is_zero, trap_blk, &[], div_blk, &[]);

    b.switch_to_block(trap_blk);
    let ba = box_f64(b, a);
    let bd = box_f64(b, bb);
    let _ = call_helper(b, cc, div_helper, &[exec_ctx, ba, bd]);
    let dummy = b.ins().f64const(0.0);
    b.ins().jump(cont, &[dummy.into()]);

    b.switch_to_block(div_blk);
    let r = b.ins().fdiv(a, bb);
    b.ins().jump(cont, &[r.into()]);

    b.switch_to_block(cont);
    b.block_params(cont)[0]
}

/// The register an opcode defines, if any (mirrors `kinds::apply_kinds`'s dest
/// conventions). Used by [`check_float_writes`].
fn reg_write_dest(op: OpCode, code: &[u16], ip: usize) -> Option<usize> {
    use OpCode::*;
    let d0 = (code[ip] >> 8) as usize;
    match op {
        // Dest packed in the SECOND word.
        Call | CallSelf | BuildArray | BuildObjectWithShape => Some((code[ip + 1] >> 8) as usize),
        // Dest in the opcode word.
        LoadIntZero | LoadIntOne | LoadIntMinusOne | LoadInt | LoadTrue | LoadFalse | LoadConst
        | LoadNull | AddInt | SubInt | MulInt | AddImm | SubImm | ModInt | PowInt | ArrayLength
        | LtInt | LteInt | GtInt | GteInt | EqInt | NeqInt | AddFloat | SubFloat | MulFloat
        | DivFloat | ModFloat | PowFloat | LtFloat | GtFloat | LteFloat | GteFloat | EqFloat
        | NeqFloat | Lt | Lte | Gt | Gte | Eq | Neq | Not | IsNull | IsArray | Instanceof | In
        | Move | ArrayGetIndex | GetFixedField | GetProperty | LoadGlobalIdx | Add | Sub | Mul
        | Div | Mod | Pow | Negate | Typeof | ToString | GetSymbol | BitAnd | BitOr | BitXor
        | Shl | Shr | Ushr | StrSlice | StrLength | ArrayPop | BuildObject | StrConcat | BuildStr
        | MakeEnumVariant | CallNativeOp => Some(d0),
        _ => None,
    }
}

/// Ops whose result the float lowering can place directly into an `F64`
/// Variable. Every other op writing a float register would `def_var` an `i64`
/// into an `F64` Variable — a Cranelift type-mismatch panic — so it is bailed
/// up front by [`check_float_writes`].
fn is_supported_float_writer(op: OpCode) -> bool {
    matches!(
        op,
        OpCode::AddFloat
            | OpCode::SubFloat
            | OpCode::MulFloat
            | OpCode::DivFloat
            | OpCode::ModFloat
            | OpCode::PowFloat
            | OpCode::Move
            | OpCode::LoadConst
    )
}

/// Reject a function up front if any `Float`-typed register is written by an
/// op the float lowering doesn't handle (loads, calls, negate, generic
/// arithmetic into a float sink…). Bailing here keeps every `def_var` on an
/// `F64` Variable type-consistent; the function falls back to the template.
pub(super) fn check_float_writes(
    code: &[u16],
    pool: &[varn_types::chunk::PoolEntry],
    meta: &[RegisterMeta],
) -> Result<(), String> {
    let mut ip = 0usize;
    while ip < code.len() {
        let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
        if let Some(op) = OpCode::from_u8(code[ip] as u8) {
            if let Some(d) = reg_write_dest(op, code, ip) {
                if meta.get(d).map_or(false, |m| m.kind == SlotKind::Float)
                    && !is_supported_float_writer(op)
                {
                    return Err(format!(
                        "clif: float register r{d} written by unsupported op {op:?}"
                    ));
                }
            }
        }
        ip += info.len;
    }
    Ok(())
}
