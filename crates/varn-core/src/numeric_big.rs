//! `bigint` semantics (spec §6–§7, ADR-0016): literals, and the division
//! rules shared with `int` (truncating `/`, dividend-signed `%`).

use crate::IntDivFault;
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
    fn division_truncates_like_int() {
        let (a, b) = (BigInt::from(-7), BigInt::from(2));
        assert_eq!(div_big(&a, &b), Ok(BigInt::from(-3)));
        assert_eq!(rem_big(&a, &b), Ok(BigInt::from(-1)));
        assert_eq!(div_big(&a, &BigInt::zero()), Err(IntDivFault::DivisionByZero));
    }
}
