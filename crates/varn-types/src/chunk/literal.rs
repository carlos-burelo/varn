//! Constant-pool literals and the serde plumbing for the interned strings they
//! hold.

use std::sync::Arc;

/// Serde variant labels for [`Literal`], one per kind, sourced from the single
/// canonical `RuntimeKind` names. Round-trip keys on the numeric index, so
/// these are identifiers only — but they stay the one canonical representation.
static LITERAL_VARIANTS: [&str; 9] = [
    varn_core::RuntimeKind::Null.name(),
    varn_core::RuntimeKind::Bool.name(),
    varn_core::RuntimeKind::Int.name(),
    varn_core::RuntimeKind::Float.name(),
    varn_core::RuntimeKind::Str.name(),
    varn_core::RuntimeKind::BigInt.name(),
    varn_core::RuntimeKind::Decimal.name(),
    varn_core::RuntimeKind::Symbol.name(),
    varn_core::RuntimeKind::Char.name(),
];

pub(super) mod rc_str_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::sync::Arc;
    pub fn serialize<S: Serializer>(s: &Arc<str>, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Arc<str>, D::Error> {
        Ok(Arc::from(String::deserialize(de)?))
    }
}

pub(super) mod opt_rc_str_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::sync::Arc;
    pub fn serialize<S: Serializer>(s: &Option<Arc<str>>, ser: S) -> Result<S::Ok, S::Error> {
        match s {
            Some(v) => ser.serialize_some(v.as_ref()),
            None => ser.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Arc<str>>, D::Error> {
        Ok(Option::<String>::deserialize(de)?.map(Arc::from))
    }
}

#[derive(Clone, Debug)]
pub enum Literal {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Arc<str>),
    BigInt(num_bigint::BigInt),
    Decimal(bigdecimal::BigDecimal),
    Symbol(crate::value::RuntimeSymbol),
    Char(char),
}

impl PartialEq for Literal {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            // Bit identity: `-0.0` and `0.0` are distinct constants (IEEE 754).
            (Self::Float(a), Self::Float(b)) => a.to_bits() == b.to_bits(),
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::BigInt(a), Self::BigInt(b)) => a == b,
            // Representation identity: `1.0d` and `1.00d` print differently.
            (Self::Decimal(a), Self::Decimal(b)) => {
                a.as_bigint_and_scale() == b.as_bigint_and_scale()
            }
            (Self::Symbol(a), Self::Symbol(b)) => a == b,
            (Self::Char(a), Self::Char(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for Literal {}

impl serde::Serialize for Literal {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        match self {
            Literal::Null => ser.serialize_unit_variant("Literal", 0, LITERAL_VARIANTS[0]),
            Literal::Bool(b) => ser.serialize_newtype_variant("Literal", 1, LITERAL_VARIANTS[1], b),
            Literal::Int(i) => ser.serialize_newtype_variant("Literal", 2, LITERAL_VARIANTS[2], i),
            Literal::Float(f) => {
                ser.serialize_newtype_variant("Literal", 3, LITERAL_VARIANTS[3], f)
            }
            Literal::Str(s) => {
                ser.serialize_newtype_variant("Literal", 4, LITERAL_VARIANTS[4], s.as_ref())
            }
            Literal::BigInt(n) => {
                ser.serialize_newtype_variant("Literal", 5, LITERAL_VARIANTS[5], n)
            }
            Literal::Decimal(d) => ser.serialize_newtype_variant(
                "Literal",
                6,
                LITERAL_VARIANTS[6],
                &d.to_plain_string(),
            ),
            Literal::Symbol(s) => {
                ser.serialize_newtype_variant("Literal", 7, LITERAL_VARIANTS[7], s)
            }
            Literal::Char(c) => ser.serialize_newtype_variant("Literal", 8, LITERAL_VARIANTS[8], c),
        }
    }
}

impl<'de> serde::Deserialize<'de> for Literal {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        use serde::de::{self, EnumAccess, VariantAccess, Visitor};
        use std::fmt;

        struct LiteralVisitor;

        impl<'de> Visitor<'de> for LiteralVisitor {
            type Value = Literal;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("Literal enum")
            }
            fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
                let (idx, variant): (u32, _) = data.variant()?;
                match idx {
                    0 => {
                        variant.unit_variant()?;
                        Ok(Literal::Null)
                    }
                    1 => Ok(Literal::Bool(variant.newtype_variant()?)),
                    2 => Ok(Literal::Int(variant.newtype_variant()?)),
                    3 => Ok(Literal::Float(variant.newtype_variant()?)),
                    4 => Ok(Literal::Str(Arc::from(
                        variant.newtype_variant::<String>()?,
                    ))),
                    5 => Ok(Literal::BigInt(variant.newtype_variant()?)),
                    6 => {
                        let text: String = variant.newtype_variant()?;
                        text.parse()
                            .map(Literal::Decimal)
                            .map_err(|_| de::Error::custom("malformed decimal literal"))
                    }
                    7 => Ok(Literal::Symbol(variant.newtype_variant()?)),
                    8 => Ok(Literal::Char(variant.newtype_variant()?)),
                    _ => Err(de::Error::unknown_variant(
                        &idx.to_string(),
                        &["0", "1", "2", "3", "4", "5", "6", "7", "8"],
                    )),
                }
            }
        }

        de.deserialize_enum("Literal", &LITERAL_VARIANTS, LiteralVisitor)
    }
}

impl std::hash::Hash for Literal {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Null => {}
            Self::Bool(b) => b.hash(state),
            Self::Int(i) => i.hash(state),
            Self::Float(f) => f.to_bits().hash(state),
            Self::Str(s) => s.hash(state),
            Self::BigInt(b) => b.hash(state),
            Self::Decimal(d) => d.as_bigint_and_scale().hash(state),
            Self::Symbol(s) => s.hash(state),
            Self::Char(c) => c.hash(state),
        }
    }
}
