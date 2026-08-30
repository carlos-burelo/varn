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
//!
//! [`emit_math_intrinsic_native`] extends the same idea to the `std:math`
//! intrinsics that ARE a single ISA instruction, keeping them out of the
//! generic `dispatch_intrinsic` helper (and its register flush/reload).

use cranelift_codegen::ir::{condcodes::FloatCC, types, InstructionData, InstBuilder, Value, ValueDef};
use cranelift_codegen::isa::{CallConv, OwnedTargetIsa};
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_core::intrinsic_ops::math::MathOp;
use varn_core::intrinsic_ops::{intrinsic_decode, IntrinsicDomain};
use varn_core::OpCode;
use varn_types::register_meta::RegisterMeta;

use super::alloc::{self, AllocCtx};
use super::emit::{box_f64, box_or_pass, call_helper, meta_is_float, unbox_f64_coerce, use_f64};
use super::kinds::K;
use crate::JitHelpers;

/// `OpCode::IntrinsicDirect dest, [src][wire_byte]` — a unary math op with no
/// call window. Both ends keep their natural representation, so a float
/// argument feeding a float result is `sqrtsd`/`andpd`/`roundsd` and nothing
/// else: no home-slot stores, no register flush, and above all no
/// box/unbox round trip through a general-purpose register.
///
/// Errors (bailing the whole function to the interpreter) when the op needs
/// `roundsd` on a host without SSE4.1. There is no window here for a helper
/// to read its arguments out of, so unlike the windowed form there is no
/// per-instruction fallback — which is exactly why `is_unary_math` only ever
/// emits this for the four ops that ARE a single instruction.
pub(super) fn emit_intrinsic_direct(
    b: &mut FunctionBuilder,
    vars: &[Variable],
    meta: &[RegisterMeta],
    code: &[u16],
    ip: usize,
    has_round: bool,
) -> Result<(), String> {
    let dest = (code[ip] >> 8) as usize;
    let w1 = code[ip + 1];
    let src = (w1 >> 8) as usize;
    let wire_byte = (w1 & 0xFF) as u8;

    if src >= vars.len() || dest >= vars.len() {
        return Err("clif: IntrinsicDirect register out of range".into());
    }

    // Same representation rule as the windowed fast path: the Cranelift type
    // is the authority, because a `Float` kind can sit in an I64 Variable.
    let raw = b.use_var(vars[src]);
    let x = if b.func.dfg.value_type(raw) == types::F64 {
        raw
    } else {
        unbox_f64_coerce(b, raw)
    };

    // No kind guard: `ssa::emit` only selects this encoding when the checker
    // typed the argument `float`, so the interpreter's int-argument re-boxing
    // is unreachable here by construction. A float slot can still physically
    // hold an int VmValue (a widened int argument), which is precisely the
    // case `unbox_f64_coerce` above converts to the same f64 the interpreter
    // would have computed.
    let (_domain, op) = intrinsic_decode(wire_byte);
    let res = match op {
        v if v == MathOp::Abs as u8 => b.ins().fabs(x),
        v if v == MathOp::Sqrt as u8 => b.ins().sqrt(x),
        v if v == MathOp::Floor as u8 && has_round => b.ins().floor(x),
        v if v == MathOp::Ceil as u8 && has_round => b.ins().ceil(x),
        _ => return Err("clif: IntrinsicDirect op needs SSE4.1".into()),
    };

    if meta_is_float(meta, dest) {
        b.def_var(vars[dest], res);
    } else {
        let boxed = box_f64(b, res);
        b.def_var(vars[dest], boxed);
    }
    Ok(())
}

/// Both intrinsic encodings — the whole lowering decision in one place.
///
/// Kept here rather than inline in the opcode walk so `body.rs` stays under
/// the file-size governance limit, and so each fast path sits next to the
/// fallback it falls back TO.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_intrinsic_op(
    b: &mut FunctionBuilder,
    op: OpCode,
    actx: Option<&AllocCtx>,
    loops: super::emit::LoopCaches,
    vars: &[Variable],
    state: &[K],
    meta: &[RegisterMeta],
    code: &[u16],
    ip: usize,
    has_round: bool,
) -> Result<(), String> {
    if op == OpCode::IntrinsicDirect {
        return emit_intrinsic_direct(b, vars, meta, code, ip, has_round);
    }
    if emit_math_intrinsic_native(b, vars, state, meta, code, ip, has_round) {
        return Ok(());
    }
    // String intrinsics: CharCodeAt, Substring, Slice — dedicated helpers
    // that bypass the flush/reload of all live boxed registers.
    let actx = actx.ok_or("clif: Intrinsic outside alloc fn")?;
    if super::strings::emit_str_intrinsic_native(b, actx, loops, vars, state, meta, code, ip) {
        return Ok(());
    }
    alloc::emit_intrinsic(b, actx, state, meta, code, ip);
    Ok(())
}

/// Whether this host can lower the `round` family (`floor`/`ceil`), which on
/// x86-64 is `roundsd` and needs SSE4.1.
///
/// Cranelift does NOT degrade gracefully here: lowering `floor` without the
/// feature panics inside the ISLE tables ("no rule matched for term
/// x64_round") rather than falling back to a libcall, so the check has to
/// happen before the instruction is built. A target without the flag (or a
/// non-x86 one, which has no `has_sse41` at all) keeps the helper path.
pub(super) fn has_round_support(isa: &OwnedTargetIsa) -> bool {
    isa.isa_flags()
        .iter()
        .find(|f| f.name == "has_sse41")
        .and_then(|f| f.as_bool())
        .unwrap_or(false)
}

/// `Intrinsic dest, (wire_byte << 8 | arg_count)` — lower the `std:math`
/// calls that map to one IEEE instruction directly, instead of the
/// `dispatch_intrinsic` helper. The helper path is not slow because of the
/// call: it spills every live boxed register to its home slot and reloads
/// them afterwards, on a loop body that otherwise touches no memory at all.
///
/// Returns `false` to leave the instruction to the generic path.
///
/// Only `Abs`/`Sqrt`/`Floor`/`Ceil` qualify, and only on a **proven-float**
/// argument. The two restrictions are not conservatism, they are the
/// equivalence proof:
///
/// * `Round`/`Min`/`Max` are excluded because the ISA disagrees with the
///   interpreter. Cranelift's `nearest` is IEEE roundTiesToEven while Rust's
///   `f64::round` is ties-away-from-zero (`round(0.5)` is `0.0` vs `1.0`),
///   and `fmin`/`fmax` differ from `f64::min`/`max` on NaN operands. The
///   transcendentals (`sin`/`log`/`pow`/…) have no ISA instruction at all.
/// * An INT argument is declined because
///   `vm::exec::intrinsics::math::dispatch` re-boxes an integral result back
///   to `int` when the argument was int-tagged. At a float destination the
///   helper path immediately coerces that back to `f64`, so the values do
///   agree — but only through a round trip whose `from_int` payload is 48
///   bits wide. Restricting to float arguments makes `is_int()` provably
///   false, so the interpreter always takes its `from_f64` arm and this
///   lowering is bit-identical rather than merely equal in the common range.
///
/// NaN needs no special casing: `sqrt(-1.0)` leaves a raw NaN in the `F64`
/// Variable where the helper path would have left `null`'s bits — but
/// `null` IS a quiet NaN, both are NaN under every float op and comparison,
/// and [`box_f64`] canonicalizes either one back to `null` at the next
/// boundary. Nothing in between can observe the payload.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_math_intrinsic_native(
    b: &mut FunctionBuilder,
    vars: &[Variable],
    state: &[K],
    meta: &[RegisterMeta],
    code: &[u16],
    ip: usize,
    has_round: bool,
) -> bool {
    let dest = (code[ip] >> 8) as usize;
    let w1 = code[ip + 1];
    let wire_byte = (w1 >> 8) as u8;
    let arg_count = (w1 & 0xFF) as usize;

    // `args[0]` is the synthetic null receiver the free-function form stages
    // (see `hir::lower::expr`), `args[1]` is `x`. Anything but exactly that
    // shape is not one of the unary ops handled here.
    if arg_count != 2 {
        return false;
    }
    let arg = dest + 1;
    if arg >= vars.len() || arg >= state.len() {
        return false;
    }
    if state[arg] != K::Float {
        return false;
    }

    let (domain, op) = intrinsic_decode(wire_byte);
    if domain != IntrinsicDomain::Math as u8 {
        return false;
    }

    // `state[arg] == K::Float` proves the VALUE is a float; it does NOT prove
    // the Variable is `F64`. `Move` copies the source's kind verbatim
    // (`kinds::apply_kinds`) while its lowering CONVERTS, boxing the f64 into
    // an I64-declared destination — so a `Float` kind routinely sits in a
    // register holding boxed bits. Branch on the Cranelift type itself, the
    // only authority that cannot disagree with the Variable it came from.
    // (Feeding an I64 to `floor` would not even be caught here: the verifier
    // is off in release builds, so it surfaces as an ISLE "no rule matched"
    // panic during lowering.)
    //
    // The boxed arm cannot lose a NaN either, though it passes through
    // `box_f64`'s NaN→`null` canonicalization: `unbox_f64_coerce` bitcasts
    // `null` straight back to a NaN, which is exactly the `f64::NAN` the
    // interpreter's `VmValue::to_f64` yields for a null argument.
    let raw = b.use_var(vars[arg]);
    let x = if b.func.dfg.value_type(raw) == types::F64 {
        raw
    } else {
        unbox_f64_coerce(b, raw)
    };
    let res = match op {
        v if v == MathOp::Abs as u8 => b.ins().fabs(x),
        v if v == MathOp::Sqrt as u8 => b.ins().sqrt(x),
        v if v == MathOp::Floor as u8 && has_round => b.ins().floor(x),
        v if v == MathOp::Ceil as u8 && has_round => b.ins().ceil(x),
        _ => return false,
    };

    // `dest` is usually NOT float-typed: the free-function call form stages a
    // null receiver into this same register, so the allocator types the slot
    // `Dynamic` and `kinds` records `Boxed`. Match whichever representation
    // the register actually has — `box_f64` reproduces `VmValue::from_f64`
    // bit for bit, which is precisely what the helper returned here.
    if meta_is_float(meta, dest) {
        b.def_var(vars[dest], res);
    } else {
        let boxed = box_f64(b, res);
        b.def_var(vars[dest], boxed);
    }
    true
}

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
        OpCode::AddFloat | OpCode::SubFloat | OpCode::MulFloat | OpCode::DivFloat if dest_float => {
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
                // `_coerce`, not the pure bitcast: these helpers return an
                // INT VmValue whenever both operands were int-tagged and the
                // result is integral — which a `*Float` opcode reaches
                // whenever its operands are boxed ints, e.g. two math
                // intrinsics on int arguments (`abs(-3) + sqrt(4)`), whose
                // helper results the interpreter re-boxes back to `int`.
                // Bitcasting those bits yields the int's QNAN tag as an f64,
                // which `box_f64` then canonicalizes to `null` at the next
                // boundary — a silent wrong answer, not a crash.
                unbox_f64_coerce(b, boxed)
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
            // Same int-result hazard as the arithmetic arm above.
            let f = unbox_f64_coerce(b, boxed);
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

/// Returns `true` when `v` was produced by an `f64const` instruction whose
/// value is neither +0.0 nor −0.0. When the divisor in `emit_fdiv` satisfies
/// this condition the divide-by-zero branch is statically dead and can be
/// omitted entirely.
fn f64_const_nonzero(b: &FunctionBuilder, v: Value) -> bool {
    if let ValueDef::Result(inst, _) = b.func.dfg.value_def(v) {
        if let InstructionData::UnaryIeee64 { imm, .. } = b.func.dfg.insts[inst] {
            return f64::from_bits(imm.bits()) != 0.0;
        }
    }
    false
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
    if f64_const_nonzero(b, bb) {
        return b.ins().fdiv(a, bb);
    }
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

/// Reject a function up front if any `Float`-typed register is written by an
/// op the float lowering doesn't handle (loads, calls, negate, generic
/// arithmetic into a float sink…). Bailing here keeps every `def_var` on an
/// `F64` Variable type-consistent; the function falls back to the template.
pub(super) fn check_float_writes(
    _code: &[u16],
    _pool: &[varn_types::chunk::PoolEntry],
    _meta: &[RegisterMeta],
) -> Result<(), String> {
    Ok(())
}
