use crate::lang_type::{BuiltinType, LangPrimitive};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]

pub enum TypeKind<T, N, C, F, O, E = ()> {
    Primitive(LangPrimitive),
    Builtin(BuiltinType),
    This,
    Array(T),
    Union(C),
    Intersection(C),
    Tuple(C),
    Named(N, Option<N>),
    Generic(N, C, Option<N>),

    TemplateLiteral(C),
    Fn(F),
    Object(O),
    Typeof(E),

    KeyOf(T),

    IndexedAccess {
        object: T,
        index: T,
    },

    Mapped {
        key_var: N,
        source: T,
        value: T,
        optional: bool,
        readonly: bool,
    },

    Conditional {
        check: T,
        extends: T,
        true_type: T,
        false_type: T,
    },

    Infer(N),

    EnumVariant {
        enum_name: N,
        variant_name: N,
        type_args: C,
        payload_ty: T,
    },

    TypePredicate {
        parameter_name: N,
        target_type: T,
    },
}

impl<T, N, C, F, O, E> TypeKind<T, N, C, F, O, E> {
    pub fn is_primitive(&self) -> bool {
        match self {
            TypeKind::Primitive(_) => true,
            TypeKind::This => true,
            _ => false,
        }
    }

    /// The language-vocabulary name of a primitive or builtin kind.
    pub fn lang_name(&self) -> Option<&'static str> {
        match self {
            TypeKind::Primitive(p) => Some(p.name()),
            TypeKind::Builtin(b) => Some(b.name()),
            _ => None,
        }
    }

    /// The kind a bare type name denotes when it is part of the language
    /// vocabulary (`int`, `Bytes`), before any user declaration is consulted.
    pub fn of_lang_name(name: &str) -> Option<Self> {
        LangPrimitive::from_str(name)
            .map(TypeKind::Primitive)
            .or_else(|| BuiltinType::from_str(name).map(TypeKind::Builtin))
    }
}
