use crate::lang_type::{BuiltinType, LangPrimitive};


#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum TypeLiteral<N> {
    Int(i64),
    Str(N),
    Bool(bool),
    Char(char),
}

impl<N> TypeLiteral<N> {
    
    pub const fn base(&self) -> LangPrimitive {
        match self {
            TypeLiteral::Int(_) => LangPrimitive::Int,
            TypeLiteral::Str(_) => LangPrimitive::Str,
            TypeLiteral::Bool(_) => LangPrimitive::Bool,
            TypeLiteral::Char(_) => LangPrimitive::Char,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]

pub enum TypeKind<T, N, C, F, O, E = ()> {
    Primitive(LangPrimitive),
    Builtin(BuiltinType),
    Literal(TypeLiteral<N>),
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
        matches!(self, TypeKind::Primitive(_) | TypeKind::This)
    }

    
    pub fn lang_name(&self) -> Option<&'static str> {
        match self {
            TypeKind::Primitive(p) => Some(p.name()),
            TypeKind::Builtin(b) => Some(b.name()),
            TypeKind::Literal(l) => Some(l.base().name()),
            _ => None,
        }
    }

    
    
    pub fn of_lang_name(name: &str) -> Option<Self> {
        LangPrimitive::from_str(name)
            .map(TypeKind::Primitive)
            .or_else(|| BuiltinType::from_str(name).map(TypeKind::Builtin))
    }
}
