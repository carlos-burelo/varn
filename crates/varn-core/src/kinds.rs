use varn_base::TypeTag;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]

pub enum TypeKind<T, N, C, F, O, E = ()> {
    Intrinsic(TypeTag),
    This,
    Array(T),
    Union(C),
    Intersection(C),
    Tuple(C),
    Named(N, Option<N>),
    Generic(N, C, Option<N>),
    LiteralInt(i64),
    LiteralFloat(u64),
    LiteralStr(N),
    LiteralBool(bool),

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrimitiveType(pub TypeTag);

impl From<TypeTag> for PrimitiveType {
    fn from(tag: TypeTag) -> Self {
        Self(tag)
    }
}

impl PrimitiveType {
    pub const COUNT: usize = 11;

    pub fn from_str(name: &str) -> Option<Self> {
        TypeTag::from_str(name).map(Self)
    }

    pub fn as_str(&self) -> &'static str {
        self.0.name()
    }
}

impl<T, N, C, F, O, E> TypeKind<T, N, C, F, O, E> {
    pub fn is_primitive(&self) -> bool {
        match self {
            TypeKind::Intrinsic(tag) => tag.is_primitive(),
            TypeKind::This => true,
            _ => false,
        }
    }
}
