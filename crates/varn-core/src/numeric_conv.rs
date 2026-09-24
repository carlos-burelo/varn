//! Conversiones numéricas explícitas (`as`). Una tabla, consumida igual por
//! const-fold, VM y JIT: un `as` que cambia de dominio nunca es un `Move`.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NumericDomain {
    Int,
    Float,
    BigInt,
    Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum NumConv {
    IntToFloat = 0,
    FloatToInt = 1,
    IntToBigInt = 2,
    BigIntToInt = 3,
    IntToDecimal = 4,
    DecimalToInt = 5,
    /// `dynamic as int`: the source domain is only known at run time.
    DynToInt = 6,
    /// `dynamic as float`.
    DynToFloat = 7,
    BigIntToFloat = 8,
    FloatToBigInt = 9,
    DecimalToFloat = 10,
    FloatToDecimal = 11,
    BigIntToDecimal = 12,
    DecimalToBigInt = 13,
}

impl NumConv {
    pub const ALL: [Self; 14] = [
        Self::IntToFloat,
        Self::FloatToInt,
        Self::IntToBigInt,
        Self::BigIntToInt,
        Self::IntToDecimal,
        Self::DecimalToInt,
        Self::DynToInt,
        Self::DynToFloat,
        Self::BigIntToFloat,
        Self::FloatToBigInt,
        Self::DecimalToFloat,
        Self::FloatToDecimal,
        Self::BigIntToDecimal,
        Self::DecimalToBigInt,
    ];

    pub const fn from_u8(raw: u8) -> Option<Self> {
        if (raw as usize) < Self::ALL.len() {
            Some(Self::ALL[raw as usize])
        } else {
            None
        }
    }

    pub const fn between(from: NumericDomain, to: NumericDomain) -> Option<Self> {
        use NumericDomain::*;
        Some(match (from, to) {
            (Int, Float) => Self::IntToFloat,
            (Float, Int) => Self::FloatToInt,
            (Int, BigInt) => Self::IntToBigInt,
            (BigInt, Int) => Self::BigIntToInt,
            (Int, Decimal) => Self::IntToDecimal,
            (Decimal, Int) => Self::DecimalToInt,
            (BigInt, Float) => Self::BigIntToFloat,
            (Float, BigInt) => Self::FloatToBigInt,
            (Decimal, Float) => Self::DecimalToFloat,
            (Float, Decimal) => Self::FloatToDecimal,
            (BigInt, Decimal) => Self::BigIntToDecimal,
            (Decimal, BigInt) => Self::DecimalToBigInt,
            _ => return None,
        })
    }

    /// Whether the conversion can raise, i.e. whether DCE must keep it.
    pub const fn can_fault(self) -> bool {
        matches!(
            self,
            Self::FloatToInt
                | Self::BigIntToInt
                | Self::DecimalToInt
                | Self::DynToInt
                | Self::FloatToBigInt
                | Self::FloatToDecimal
        )
    }
}

/// `f as int`: truncates toward zero; `None` for NaN, ±Inf or out of range.
pub fn float_to_int(f: f64) -> Option<i64> {
    const TWO_POW_63: f64 = 9_223_372_036_854_775_808.0;
    let t = f.trunc();
    if t.is_nan() || t < -TWO_POW_63 || t >= TWO_POW_63 {
        return None;
    }
    Some(t as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_to_int_truncates_and_rejects_non_representable() {
        assert_eq!(float_to_int(3.9), Some(3));
        assert_eq!(float_to_int(-3.9), Some(-3));
        assert_eq!(float_to_int(-9_223_372_036_854_775_808.0), Some(i64::MIN));
        assert_eq!(float_to_int(9_223_372_036_854_775_808.0), None);
        assert_eq!(float_to_int(f64::NAN), None);
        assert_eq!(float_to_int(f64::INFINITY), None);
        assert_eq!(float_to_int(f64::NEG_INFINITY), None);
    }

    #[test]
    fn wire_roundtrip() {
        for c in NumConv::ALL {
            assert_eq!(NumConv::from_u8(c as u8), Some(c));
        }
        assert_eq!(NumConv::from_u8(NumConv::ALL.len() as u8), None);
    }
}
