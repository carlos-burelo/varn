//! Constant-pool entries: literals, nested function protos, class definitions.

use std::rc::Rc;

use super::literal::Literal;
use crate::FunctionProto;

#[derive(Clone)]
pub enum PoolEntry {
    Literal(Literal),
    Function(std::rc::Rc<FunctionProto>),
    Shape(Vec<std::rc::Rc<str>>),
}

impl std::fmt::Debug for PoolEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Literal(lit) => f.debug_tuple("Literal").field(lit).finish(),
            Self::Function(proto) => f
                .debug_struct("Function")
                .field("name", &proto.name)
                .field("arity", &proto.arity)
                .finish_non_exhaustive(),
            Self::Shape(keys) => f.debug_tuple("Shape").field(keys).finish(),
        }
    }
}

impl std::hash::Hash for PoolEntry {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Literal(lit) => lit.hash(state),
            Self::Function(f) => {
                std::rc::Rc::as_ptr(f).hash(state);
            }
            Self::Shape(keys) => keys.hash(state),
        }
    }
}

impl PartialEq for PoolEntry {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Literal(l1), Self::Literal(l2)) => l1 == l2,
            (Self::Shape(k1), Self::Shape(k2)) => k1 == k2,
            (Self::Function(f1), Self::Function(f2)) => std::rc::Rc::ptr_eq(f1, f2),
            _ => false,
        }
    }
}

impl Eq for PoolEntry {}

impl serde::Serialize for PoolEntry {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        match self {
            PoolEntry::Literal(l) => ser.serialize_newtype_variant("PoolEntry", 0, "Literal", l),
            PoolEntry::Function(f) => {
                ser.serialize_newtype_variant("PoolEntry", 1, "Function", f.as_ref())
            }
            PoolEntry::Shape(keys) => {
                let strs: Vec<&str> = keys.iter().map(|s| s.as_ref()).collect();
                ser.serialize_newtype_variant("PoolEntry", 2, "Shape", &strs)
            }
        }
    }
}

impl<'de> serde::Deserialize<'de> for PoolEntry {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        use serde::de::{self, EnumAccess, VariantAccess, Visitor};
        use std::fmt;

        struct PoolEntryVisitor;

        impl<'de> Visitor<'de> for PoolEntryVisitor {
            type Value = PoolEntry;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("PoolEntry enum")
            }
            fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
                let (idx, variant): (u32, _) = data.variant()?;
                match idx {
                    0 => Ok(PoolEntry::Literal(variant.newtype_variant()?)),
                    1 => Ok(PoolEntry::Function(Rc::new(
                        variant.newtype_variant::<FunctionProto>()?,
                    ))),
                    2 => {
                        let strs = variant.newtype_variant::<Vec<String>>()?;
                        Ok(PoolEntry::Shape(
                            strs.into_iter().map(|s| Rc::from(s)).collect(),
                        ))
                    }
                    _ => Err(de::Error::unknown_variant(
                        &idx.to_string(),
                        &["0", "1", "2"],
                    )),
                }
            }
        }

        de.deserialize_enum(
            "PoolEntry",
            &["Literal", "Function", "Shape"],
            PoolEntryVisitor,
        )
    }
}

impl PoolEntry {
    pub fn as_str(&self) -> Option<&str> {
        if let PoolEntry::Literal(Literal::Str(s)) = self {
            Some(s.as_ref())
        } else {
            None
        }
    }
}

