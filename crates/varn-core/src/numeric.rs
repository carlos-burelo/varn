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
//! - Every arithmetic operator keeps its operands' domain (spec §10): there
//!   is no operator whose result class differs from its operands.
//! - `int / int` is an `int` truncated toward zero; a zero divisor raises
//!   `DivisionByZero` and `INT_MIN / -1` raises `IntegerOverflow`.
//! - `int % int` has the dividend's sign; `INT_MIN % -1 == 0`.
//! - `int ** int` produces an `int` (wrapping). A negative exponent
//!   raises a runtime error instead of silently producing a float.
//! - `decimal` absorbs `int` (exact). Mixed `int`/`float` and
//!   `decimal`/`float` operands are a checker error (spec §9); an integer
//!   *literal* adopts the other operand's type when exactly representable
//!   (`varn_checker::types::numeric_literal`).

/// The largest and smallest values Varn's `int` can hold.
pub const INT_MAX: i64 = i64::MAX;
pub const INT_MIN: i64 = i64::MIN;

/// `v` as an `int`. Every 64-bit integer is a valid `int`.
#[inline(always)]
pub fn checked_int(v: i64) -> Option<i64> {
    Some(v)
}

/// `a + b` as an `int`, or `None` on overflow.
#[inline(always)]
pub fn add_int(a: i64, b: i64) -> Option<i64> {
    a.checked_add(b)
}

/// `a - b` as an `int`, or `None` on overflow.
#[inline(always)]
pub fn sub_int(a: i64, b: i64) -> Option<i64> {
    a.checked_sub(b)
}

/// `a * b` as an `int`, or `None` on overflow.
#[inline(always)]
pub fn mul_int(a: i64, b: i64) -> Option<i64> {
    a.checked_mul(b)
}

/// `a ** e` as an `int`, or `None` on overflow.
#[inline(always)]
pub fn pow_int(a: i64, e: u32) -> Option<i64> {
    a.checked_pow(e)
}

/// `-a` as an `int`, or `None` on overflow (when `a == i64::MIN`).
#[inline(always)]
pub fn neg_int(a: i64) -> Option<i64> {
    a.checked_neg()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntDivFault {
    DivisionByZero,
    Overflow,
}

/// `a / b` truncated toward zero.
#[inline(always)]
pub fn div_int(a: i64, b: i64) -> Result<i64, IntDivFault> {
    if b == 0 {
        return Err(IntDivFault::DivisionByZero);
    }
    a.checked_div(b).ok_or(IntDivFault::Overflow)
}

/// Remainder with the dividend's sign. `MIN % -1` is exactly 0: the quotient
/// overflows, the remainder does not.
#[inline(always)]
pub fn rem_int(a: i64, b: i64) -> Result<i64, IntDivFault> {
    if b == 0 {
        return Err(IntDivFault::DivisionByZero);
    }
    Ok(a.wrapping_rem(b))
}

/// `a / b` rounded toward negative infinity.
pub fn floor_div_int(a: i64, b: i64) -> Result<i64, IntDivFault> {
    let q = div_int(a, b)?;
    let inexact = a.wrapping_rem(b) != 0;
    Ok(if inexact && ((a < 0) != (b < 0)) { q - 1 } else { q })
}

/// `a / b` rounded toward positive infinity.
pub fn ceil_div_int(a: i64, b: i64) -> Result<i64, IntDivFault> {
    let q = div_int(a, b)?;
    let inexact = a.wrapping_rem(b) != 0;
    Ok(if inexact && ((a < 0) == (b < 0)) { q + 1 } else { q })
}

/// Euclidean modulo: always in `0..|b|`.
pub fn mod_int(a: i64, b: i64) -> Result<i64, IntDivFault> {
    if b == 0 {
        return Err(IntDivFault::DivisionByZero);
    }
    Ok(a.wrapping_rem_euclid(b))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericOperand {
    Int,
    Float,
    Decimal,
}

/// The common operand class of a numeric binary operation, or `None` when
/// the two sides don't reduce to a single numeric class. Only `int` widens
/// implicitly, and only into the exact domain `decimal` (spec §9): an
/// `int`/`float` mix is lossy and needs an explicit `as`.
pub fn binary_operand_kind(
    l: Option<NumericOperand>,
    r: Option<NumericOperand>,
) -> Option<NumericOperand> {
    use NumericOperand::*;
    match (l?, r?) {
        (Int, Int) => Some(Int),
        (Float, Float) => Some(Float),
        (Decimal, Decimal) | (Decimal, Int) | (Int, Decimal) => Some(Decimal),
        (Int, Float) | (Float, Int) | (Decimal, Float) | (Float, Decimal) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_ceil_mod() {
        assert_eq!(floor_div_int(-7, 2), Ok(-4));
        assert_eq!(floor_div_int(7, 2), Ok(3));
        assert_eq!(ceil_div_int(7, 2), Ok(4));
        assert_eq!(ceil_div_int(-7, 2), Ok(-3));
        assert_eq!(mod_int(-7, 2), Ok(1));
        assert_eq!(mod_int(7, -2), Ok(1));
        assert_eq!(mod_int(i64::MIN, -1), Ok(0));
        assert_eq!(floor_div_int(i64::MIN, -1), Err(IntDivFault::Overflow));
        assert_eq!(ceil_div_int(1, 0), Err(IntDivFault::DivisionByZero));
    }

    #[test]
    fn int_float_mix_has_no_common_class() {
        use NumericOperand::*;
        assert_eq!(binary_operand_kind(Some(Int), Some(Float)), None);
        assert_eq!(binary_operand_kind(Some(Float), Some(Int)), None);
        assert_eq!(binary_operand_kind(Some(Int), Some(Decimal)), Some(Decimal));
    }

    #[test]
    fn div_int_truncates_toward_zero() {
        assert_eq!(div_int(7, 2), Ok(3));
        assert_eq!(div_int(-7, 2), Ok(-3));
        assert_eq!(div_int(7, -2), Ok(-3));
    }

    #[test]
    fn div_int_faults() {
        assert_eq!(div_int(1, 0), Err(IntDivFault::DivisionByZero));
        assert_eq!(div_int(i64::MIN, -1), Err(IntDivFault::Overflow));
    }

    #[test]
    fn rem_int_follows_dividend_sign_and_never_overflows() {
        assert_eq!(rem_int(-7, 2), Ok(-1));
        assert_eq!(rem_int(7, -2), Ok(1));
        assert_eq!(rem_int(i64::MIN, -1), Ok(0));
        assert_eq!(rem_int(1, 0), Err(IntDivFault::DivisionByZero));
    }
}
