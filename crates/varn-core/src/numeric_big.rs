use crate::IntDivFault;
use bigdecimal::{BigDecimal, RoundingMode};
use num_bigint::BigInt;
use num_traits::{Num, One, ToPrimitive, Zero};

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
        Some(_) | None => (10, body),
    };
    let v = BigInt::from_str_radix(digits, radix).ok()?;
    Some(if neg { -v } else { v })
}

pub fn div_big(a: &BigInt, b: &BigInt) -> Result<BigInt, IntDivFault> {
    if b.is_zero() {
        return Err(IntDivFault::DivisionByZero);
    }
    Ok(a / b)
}

pub fn rem_big(a: &BigInt, b: &BigInt) -> Result<BigInt, IntDivFault> {
    if b.is_zero() {
        return Err(IntDivFault::DivisionByZero);
    }
    Ok(a % b)
}

pub const DECIMAL_DIV_DIGITS: u64 = 34;

pub fn mul_decimal(a: &BigDecimal, b: &BigDecimal) -> BigDecimal {
    let wanted = a.fractional_digit_count() + b.fractional_digit_count();
    let product = a * b;
    if wanted > 0 && product.fractional_digit_count() < wanted {
        product.with_scale(wanted)
    } else {
        product
    }
}

pub fn div_decimal(a: &BigDecimal, b: &BigDecimal) -> Result<BigDecimal, IntDivFault> {
    if b.is_zero() {
        return Err(IntDivFault::DivisionByZero);
    }
    let digits = std::num::NonZeroU64::new(DECIMAL_DIV_DIGITS).expect("non-zero");
    Ok((a / b)
        .with_precision_round(digits, RoundingMode::HalfEven)
        .normalized_if_exact(a, b))
}

pub fn rem_decimal(a: &BigDecimal, b: &BigDecimal) -> Result<BigDecimal, IntDivFault> {
    if b.is_zero() {
        return Err(IntDivFault::DivisionByZero);
    }
    Ok(a % b)
}

pub const DECIMAL_POW_MAX_DIGITS: u64 = 1 << 22;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecimalPowFault {
    NonIntegral,
    TooLarge,
    ZeroToNegative,
}

pub fn pow_decimal(base: &BigDecimal, exp: &BigDecimal) -> Result<BigDecimal, DecimalPowFault> {
    if !exp.is_integer() {
        return Err(DecimalPowFault::NonIntegral);
    }
    let e = exp.to_i64().ok_or(DecimalPowFault::TooLarge)?;
    let one = BigDecimal::one();
    if base.is_zero() {
        return match e {
            0 => Ok(one),
            n if n > 0 => Ok(BigDecimal::zero()),
            _ => Err(DecimalPowFault::ZeroToNegative),
        };
    }
    let magnitude = e.unsigned_abs();
    if e == 0 {
        return Ok(one);
    }
    let unit = base.abs() == one;
    let scale = base.fractional_digit_count().max(0) as u64;
    if unit {
        let target = scale.saturating_mul(magnitude);
        if target > DECIMAL_POW_MAX_DIGITS {
            return Err(DecimalPowFault::TooLarge);
        }
        let sign_kept = *base < BigDecimal::zero() && magnitude % 2 == 1;
        let value = if sign_kept { -one.clone() } else { one };
        let result = value.with_scale(target as i64);
        return if e < 0 {
            div_decimal(&BigDecimal::one(), &result).map_err(|_| DecimalPowFault::ZeroToNegative)
        } else {
            Ok(result)
        };
    }
    if base.digits().saturating_mul(magnitude) > DECIMAL_POW_MAX_DIGITS {
        return Err(DecimalPowFault::TooLarge);
    }
    let mut result = one.clone();
    let mut square = base.clone();
    let mut bits = magnitude;
    while bits > 0 {
        if bits & 1 == 1 {
            result = mul_decimal(&result, &square);
        }
        bits >>= 1;
        if bits > 0 {
            square = mul_decimal(&square, &square);
        }
    }
    if e < 0 {
        div_decimal(&one, &result).map_err(|_| DecimalPowFault::ZeroToNegative)
    } else {
        Ok(result)
    }
}

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
