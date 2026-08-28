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
//! - `int` is a 64-bit two's-complement integer, range `[-2^63, 2^63-1]`.
//!   Integer arithmetic that leaves that range **raises `integer overflow`**
//!   — identically in the interpreter, the JIT (via CPU hardware overflow flags)
//!   and the constant folder. It does not wrap, does not saturate and does
//!   not promote to float. Overflow the folder can prove is a compile error
//!   rather than a runtime one.
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

/// Sign-extend the low 48 bits of `v`. This is the unboxing of an inline
/// int-tagged `VmValue` payload in the NaN-box.
#[inline(always)]
pub fn wrap_i48(v: i64) -> i64 {
    (v << 16) >> 16
}

/// The largest and smallest values Varn's `int` can hold (native 64-bit integer).
pub const INT_MAX: i64 = i64::MAX;
pub const INT_MIN: i64 = i64::MIN;

/// `v` as an `int`. Every 64-bit integer is a valid `int`.
#[inline(always)]
pub fn checked_int(v: i64) -> Option<i64> {
    Some(v)
}

#[inline(always)]
pub fn checked_i48(v: i64) -> Option<i64> {
    checked_int(v)
}

/// `a + b` as an `int`, or `None` on overflow.
#[inline(always)]
pub fn add_int(a: i64, b: i64) -> Option<i64> {
    a.checked_add(b)
}

#[inline(always)]
pub fn add_i48(a: i64, b: i64) -> Option<i64> {
    add_int(a, b)
}

/// `a - b` as an `int`, or `None` on overflow.
#[inline(always)]
pub fn sub_int(a: i64, b: i64) -> Option<i64> {
    a.checked_sub(b)
}

#[inline(always)]
pub fn sub_i48(a: i64, b: i64) -> Option<i64> {
    sub_int(a, b)
}

/// `a * b` as an `int`, or `None` on overflow.
#[inline(always)]
pub fn mul_int(a: i64, b: i64) -> Option<i64> {
    a.checked_mul(b)
}

#[inline(always)]
pub fn mul_i48(a: i64, b: i64) -> Option<i64> {
    mul_int(a, b)
}

/// `a ** e` as an `int`, or `None` on overflow.
#[inline(always)]
pub fn pow_int(a: i64, e: u32) -> Option<i64> {
    a.checked_pow(e)
}

#[inline(always)]
pub fn pow_i48(a: i64, e: u32) -> Option<i64> {
    pow_int(a, e)
}

/// `-a` as an `int`, or `None` on overflow (when `a == i64::MIN`).
#[inline(always)]
pub fn neg_int(a: i64) -> Option<i64> {
    a.checked_neg()
}

#[inline(always)]
pub fn neg_i48(a: i64) -> Option<i64> {
    neg_int(a)
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
