//! Numeric semantics of binary operators — single source of truth.
//!
//! Every layer that reasons about numeric binary operations (binder
//! inference, checker validation, expression annotations, HIR lowering,
//! VM and JIT fast paths) must derive its answer from the rules here.
//! Divergent hand-copies of these rules are how the interpreter and the
//! JIT once disagreed about the same program.
//!
//! The semantics:
//!
//! - `int` is a 48-bit two's-complement integer (the NaN-box payload),
//!   range `[-2^47, 2^47-1]`. Integer arithmetic that leaves that range
//!   **raises `integer overflow`** — identically in the interpreter, the
//!   JIT and the constant folder. It does not wrap, does not saturate and
//!   does not promote to float.
//!
//!   Wrapping is what this used to do, and it was the wrong contract: a
//!   48-bit `int` is a legitimate design, but one that answers
//!   `1000000007 * 1000000007` with a wrong number and no signal is not.
//!   The range is unchanged; only the behaviour at its edge is. Overflow
//!   the folder can prove is a compile error rather than a runtime one.
//! - `int / int` always produces a `float`. Integer division that
//!   sometimes returned `int` (when exact) made a value's runtime type
//!   depend on the values themselves, which poisons every typed fast
//!   path downstream.
//! - `int % int` produces an `int` (truncated remainder). Zero divisor
//!   raises a runtime error, as does `int / 0`.
//! - `int ** int` produces an `int` (wrapping). A negative exponent
//!   raises a runtime error instead of silently producing a float.
//! - Mixed `int`/`float` operands promote to `float`; `decimal` absorbs
//!   `int`. `decimal`/`float` mixes are a checker error and have no
//!   numeric class.

use crate::ast::operators::BinaryOp;

/// Sign-extend the low 48 bits of `v`. This is the UNBOXING of an int-tagged
/// `VmValue` payload, not an overflow policy: arithmetic that leaves the i48
/// range raises (see [`checked_i48`] and the `*_i48` operators below) instead
/// of being reduced by this function.
#[inline(always)]
pub fn wrap_i48(v: i64) -> i64 {
    (v << 16) >> 16
}

/// The largest and smallest values Varn's `int` can hold.
pub const INT_MAX: i64 = (1 << 47) - 1;
pub const INT_MIN: i64 = -(1 << 47);

/// `v` as an `int`, or `None` when it does not fit in 48 bits.
///
/// The single overflow predicate. Every tier consults it so none of them can
/// disagree about which results exist: the interpreter and the constant folder
/// call it directly, and JIT code re-derives the same test inline and diverts
/// to the interpreter helper — which calls this — to raise.
#[inline(always)]
pub fn checked_i48(v: i64) -> Option<i64> {
    if wrap_i48(v) == v {
        Some(v)
    } else {
        None
    }
}

/// `a + b` as an `int`, or `None` on overflow. Two i48 operands cannot
/// overflow an `i64`, so the sum is computed exactly and only its range is in
/// question.
#[inline(always)]
pub fn add_i48(a: i64, b: i64) -> Option<i64> {
    checked_i48(a.wrapping_add(b))
}

/// `a - b` as an `int`, or `None` on overflow. See [`add_i48`].
#[inline(always)]
pub fn sub_i48(a: i64, b: i64) -> Option<i64> {
    checked_i48(a.wrapping_sub(b))
}

/// `a * b` as an `int`, or `None` on overflow.
///
/// Unlike addition, a product of two i48 values CAN exceed `i64` (2^47 · 2^47
/// is 2^94), so the `i64` multiplication is itself checked before the range
/// test — otherwise the wrapped `i64` product could land back inside i48 and
/// read as a valid answer.
#[inline(always)]
pub fn mul_i48(a: i64, b: i64) -> Option<i64> {
    a.checked_mul(b).and_then(checked_i48)
}

/// `a ** e` as an `int`, or `None` on overflow. Checked in `i64` first, for
/// the reason given on [`mul_i48`].
#[inline(always)]
pub fn pow_i48(a: i64, e: u32) -> Option<i64> {
    a.checked_pow(e).and_then(checked_i48)
}

/// `-a` as an `int`, or `None` on overflow. The one case is `-INT_MIN`, which
/// has no i48 representation; unchecked negation used to return `INT_MIN`
/// again.
#[inline(always)]
pub fn neg_i48(a: i64) -> Option<i64> {
    checked_i48(a.wrapping_neg())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericOperand {
    Int,
    Float,
    Decimal,
}

/// The common operand class of a numeric binary operation, or `None` when
/// the two sides don't reduce to a single numeric class (unknown types,
/// strings, decimal/float mixes, ...).
pub fn binary_operand_kind(
    l: Option<NumericOperand>,
    r: Option<NumericOperand>,
) -> Option<NumericOperand> {
    use NumericOperand::*;
    match (l?, r?) {
        (Int, Int) => Some(Int),
        (Decimal, Decimal) | (Decimal, Int) | (Int, Decimal) => Some(Decimal),
        (Float, Float) | (Float, Int) | (Int, Float) => Some(Float),
        (Decimal, Float) | (Float, Decimal) => None,
    }
}

/// Result class of an arithmetic op whose operands share class `operands`.
/// `int / int → float` is the one place where the result class differs
/// from the operand class.
pub fn binary_result_kind(op: BinaryOp, operands: NumericOperand) -> NumericOperand {
    match (op, operands) {
        (BinaryOp::Div, NumericOperand::Int) => NumericOperand::Float,
        (_, k) => k,
    }
}
