//! `bigint` semantics (spec §6–§7, ADR-0016): literals, and the division
//! rules shared with `int` (truncating `/`, dividend-signed `%`).

use crate::IntDivFault;
use bigdecimal::{BigDecimal, RoundingMode};
use num_bigint::BigInt;
use num_traits::{Num, Zero};

/// The value of a `bigint` literal's digits (without the `n` suffix):
/// decimal, or `0x`/`0o`/`0b` prefixed, `_` separators allowed.
pub fn parse_bigint_literal(text: &str) -> Option<BigInt> {
    let clean: String = text.chars().filter(|c| *c != '_').collect();
    let (neg, body) = match clean.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, clean.as_str()),
    };
    let (radix, digits) = match body.get(..2) {
        Some("0x") | Some("0X") => (16, &body[2..]),
        Some("0o") | Some("0O") => (8, &body[2..]),
        Some("0b") | Some("0B") => (2, &body[2..]),
        _ => (10, body),
    };
    let v = BigInt::from_str_radix(digits, radix).ok()?;
    Some(if neg { -v } else { v })
}

/// `a / b` truncated toward zero.
pub fn div_big(a: &BigInt, b: &BigInt) -> Result<BigInt, IntDivFault> {
    if b.is_zero() {
        return Err(IntDivFault::DivisionByZero);
    }
    Ok(a / b)
}

/// Remainder with the dividend's sign.
pub fn rem_big(a: &BigInt, b: &BigInt) -> Result<BigInt, IntDivFault> {
    if b.is_zero() {
        return Err(IntDivFault::DivisionByZero);
    }
    Ok(a % b)
}

/// Significant digits a `decimal` quotient keeps (IEEE decimal128).
pub const DECIMAL_DIV_DIGITS: u64 = 34;

/// `a / b` for `decimal`: exact when the quotient terminates within
/// [`DECIMAL_DIV_DIGITS`] significant digits, rounded half-even otherwise.
pub fn div_decimal(a: &BigDecimal, b: &BigDecimal) -> Result<BigDecimal, IntDivFault> {
    if b.is_zero() {
        return Err(IntDivFault::DivisionByZero);
    }
    let digits = std::num::NonZeroU64::new(DECIMAL_DIV_DIGITS).expect("non-zero");
    Ok((a / b)
        .with_precision_round(digits, RoundingMode::HalfEven)
        .normalized_if_exact(a, b))
}

/// Remainder of `decimal` division, exact, with the dividend's sign.
pub fn rem_decimal(a: &BigDecimal, b: &BigDecimal) -> Result<BigDecimal, IntDivFault> {
    if b.is_zero() {
        return Err(IntDivFault::DivisionByZero);
    }
    Ok(a % b)
}

/// Keeps a terminating quotient free of the trailing zeros rounding pads in.
trait NormalizedIfExact {
    fn normalized_if_exact(self, a: &BigDecimal, b: &BigDecimal) -> BigDecimal;
}

impl NormalizedIfExact for BigDecimal {
    fn normalized_if_exact(self, a: &BigDecimal, b: &BigDecimal) -> BigDecimal {
        let n = self.normalized();
        if &(&n * b) == a {
            n
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals() {
        assert_eq!(parse_bigint_literal("255"), Some(BigInt::from(255)));
        assert_eq!(parse_bigint_literal("0xff"), Some(BigInt::from(255)));
        assert_eq!(parse_bigint_literal("1_000"), Some(BigInt::from(1000)));
        let big = parse_bigint_literal("170141183460469231731687303715884105728").unwrap();
        assert_eq!(big.to_string(), "170141183460469231731687303715884105728");
        assert_eq!(parse_bigint_literal("12z"), None);
    }

    #[test]
    fn decimal_division() {
        let d = |s: &str| s.parse::<BigDecimal>().unwrap();
        assert_eq!(
            div_decimal(&d("1"), &d("4")).unwrap().to_plain_string(),
            "0.25"
        );
        assert_eq!(
            div_decimal(&d("1"), &d("3")).unwrap().to_plain_string(),
            "0.3333333333333333333333333333333333"
        );
        assert_eq!(
            div_decimal(&d("1"), &d("0")),
            Err(IntDivFault::DivisionByZero)
        );
        assert_eq!(rem_decimal(&d("-7.5"), &d("2")).unwrap(), d("-1.5"));
    }

    #[test]
    fn division_truncates_like_int() {
        let (a, b) = (BigInt::from(-7), BigInt::from(2));
        assert_eq!(div_big(&a, &b), Ok(BigInt::from(-3)));
        assert_eq!(rem_big(&a, &b), Ok(BigInt::from(-1)));
        assert_eq!(
            div_big(&a, &BigInt::zero()),
            Err(IntDivFault::DivisionByZero)
        );
    }
}
