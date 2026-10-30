pub const INT_MAX: i64 = i64::MAX;
pub const INT_MIN: i64 = i64::MIN;

#[inline(always)]
pub fn checked_int(v: i64) -> Option<i64> {
    Some(v)
}

#[inline(always)]
pub fn add_int(a: i64, b: i64) -> Option<i64> {
    a.checked_add(b)
}

#[inline(always)]
pub fn sub_int(a: i64, b: i64) -> Option<i64> {
    a.checked_sub(b)
}

#[inline(always)]
pub fn mul_int(a: i64, b: i64) -> Option<i64> {
    a.checked_mul(b)
}

#[inline(always)]
pub fn pow_int(a: i64, e: u64) -> Option<i64> {
    match a {
        0 => Some(i64::from(e == 0)),
        1 => Some(1),
        -1 => Some(if e.is_multiple_of(2) { 1 } else { -1 }),
        _ => u32::try_from(e).ok().and_then(|e| a.checked_pow(e)),
    }
}

pub const FIXED_MAX_DIGITS: i64 = 100;

pub fn fixed_digits(digits: Option<i64>) -> Option<usize> {
    let d = digits.unwrap_or(0);
    if (0..=FIXED_MAX_DIGITS).contains(&d) {
        Some(d as usize)
    } else {
        None
    }
}

pub fn float_to_fixed(x: f64, digits: usize) -> String {
    if !x.is_finite() {
        return crate::float_to_string(x);
    }
    let exact = format!("{:.1100}", x.abs());
    let (whole, fraction) = exact.split_once('.').unwrap_or((exact.as_str(), ""));
    let mut kept: Vec<u8> = whole.bytes().chain(fraction.bytes().take(digits)).collect();
    let round_up = fraction.as_bytes().get(digits).is_some_and(|d| *d >= b'5');
    if round_up {
        let mut at = kept.len();
        loop {
            if at == 0 {
                kept.insert(0, b'1');
                break;
            }
            at -= 1;
            if kept[at] == b'9' {
                kept[at] = b'0';
            } else {
                kept[at] += 1;
                break;
            }
        }
    }
    let point = kept.len() - digits;
    let (int_part, frac_part) = kept.split_at(point);
    let mut out = String::new();
    if x.is_sign_negative() && x != 0.0 {
        out.push('-');
    }
    out.push_str(std::str::from_utf8(int_part).unwrap_or("0"));
    if digits > 0 {
        out.push('.');
        out.push_str(std::str::from_utf8(frac_part).unwrap_or(""));
    }
    out
}

pub const MAX_ALLOC_BYTES: usize = 1 << 31;

pub const MAX_STR_BYTES: usize = 1 << 29;

pub fn clamp_index(index: i64, len: usize) -> usize {
    if index < 0 {
        usize::try_from(index.saturating_add(len as i64)).unwrap_or(0)
    } else {
        usize::try_from(index).map_or(len, |i| i.min(len))
    }
}

pub fn slice_bounds(len: usize, start: Option<i64>, end: Option<i64>) -> (usize, usize) {
    let from = clamp_index(start.unwrap_or(0), len);
    let to = clamp_index(end.unwrap_or(len as i64), len).max(from);
    (from, to)
}

#[inline]
pub fn fmax(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() {
            b
        } else {
            a
        }
    } else if a > b {
        a
    } else {
        b
    }
}

#[inline]
pub fn fmin(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() {
            a
        } else {
            b
        }
    } else if a < b {
        a
    } else {
        b
    }
}

#[inline]
pub fn fsign(x: f64) -> f64 {
    if x.is_nan() || x == 0.0 {
        x
    } else if x > 0.0 {
        1.0
    } else {
        -1.0
    }
}

#[inline(always)]
pub fn neg_int(a: i64) -> Option<i64> {
    a.checked_neg()
}

pub const SHIFT_MAX: i64 = 63;

#[inline(always)]
pub fn shl_int(a: i64, n: i64) -> Option<i64> {
    u32::try_from(n).ok().and_then(|n| a.checked_shl(n))
}

#[inline(always)]
pub fn shr_int(a: i64, n: i64) -> Option<i64> {
    u32::try_from(n).ok().and_then(|n| a.checked_shr(n))
}

#[inline(always)]
pub fn ushr_int(a: i64, n: i64) -> Option<i64> {
    u32::try_from(n)
        .ok()
        .and_then(|n| (a as u64).checked_shr(n))
        .map(|r| r as i64)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntDivFault {
    DivisionByZero,
    Overflow,
}

#[inline(always)]
pub fn div_int(a: i64, b: i64) -> Result<i64, IntDivFault> {
    if b == 0 {
        return Err(IntDivFault::DivisionByZero);
    }
    a.checked_div(b).ok_or(IntDivFault::Overflow)
}

#[inline(always)]
pub fn rem_int(a: i64, b: i64) -> Result<i64, IntDivFault> {
    if b == 0 {
        return Err(IntDivFault::DivisionByZero);
    }
    Ok(a.wrapping_rem(b))
}

pub fn floor_div_int(a: i64, b: i64) -> Result<i64, IntDivFault> {
    let q = div_int(a, b)?;
    let inexact = a.wrapping_rem(b) != 0;
    Ok(if inexact && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    })
}

pub fn ceil_div_int(a: i64, b: i64) -> Result<i64, IntDivFault> {
    let q = div_int(a, b)?;
    let inexact = a.wrapping_rem(b) != 0;
    Ok(if inexact && ((a < 0) == (b < 0)) {
        q + 1
    } else {
        q
    })
}

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
    BigInt,
}

pub fn binary_operand_kind(
    l: Option<NumericOperand>,
    r: Option<NumericOperand>,
) -> Option<NumericOperand> {
    use NumericOperand::*;
    match (l?, r?) {
        (Int, Int) => Some(Int),
        (Float, Float) => Some(Float),
        (Decimal, Decimal) | (Decimal, Int) | (Int, Decimal) => Some(Decimal),
        (BigInt, BigInt) | (BigInt, Int) | (Int, BigInt) => Some(BigInt),
        (Int, Float)
        | (Float, Int)
        | (Decimal, Float)
        | (Float, Decimal)
        | (BigInt, Float)
        | (Float, BigInt)
        | (BigInt, Decimal)
        | (Decimal, BigInt) => None,
    }
}

pub fn float_to_string(f: f64) -> String {
    if f.is_nan() {
        "NaN".to_owned()
    } else if f.is_infinite() {
        if f > 0.0 { "Infinity" } else { "-Infinity" }.to_owned()
    } else if f != 0.0 && (f.abs() >= 1e21 || f.abs() < 1e-6) {
        format!("{f:e}")
    } else {
        let mut text = f.to_string();
        if !text.contains('.') {
            text.push_str(".0");
        }
        text
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
