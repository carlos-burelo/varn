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
pub fn pow_int(a: i64, e: u32) -> Option<i64> {
    a.checked_pow(e)
}

#[inline(always)]
pub fn neg_int(a: i64) -> Option<i64> {
    a.checked_neg()
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
