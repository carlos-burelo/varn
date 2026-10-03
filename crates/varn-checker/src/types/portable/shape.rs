use varn_core::{BuiltinType, LangPrimitive, TypeLiteral};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PortableType {
    Primitive(LangPrimitive),
    Builtin(BuiltinType),
    Literal(TypeLiteral<String>),
    This,
    Array(Box<PortableType>),
    Union(Vec<PortableType>),
    Intersection(Vec<PortableType>),
    Tuple(Vec<PortableType>),
    Named(String, Option<String>),
    Generic(String, Vec<PortableType>, Option<String>),
    TemplateLiteral(Vec<PortableType>),
    Fn(PortableFunction),
    Object(Vec<PortableObjectMember>),
    KeyOf(Box<PortableType>),
    IndexedAccess {
        object: Box<PortableType>,
        index: Box<PortableType>,
    },
    Mapped {
        key_var: String,
        source: Box<PortableType>,
        value: Box<PortableType>,
        optional: bool,
        readonly: bool,
    },
    Conditional {
        check: Box<PortableType>,
        extends: Box<PortableType>,
        true_type: Box<PortableType>,
        false_type: Box<PortableType>,
    },
    Infer(String),
    EnumVariant {
        enum_name: String,
        variant_name: String,
        type_args: Vec<PortableType>,
        payload_ty: Box<PortableType>,
    },
    TypePredicate {
        parameter_name: String,
        target_type: Box<PortableType>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PortableFunction {
    pub params: Vec<PortableParam>,
    pub return_type: Box<PortableType>,
    pub is_arrow: bool,
    pub type_params: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PortableParam {
    pub name: Option<String>,
    pub ty: PortableType,
    pub optional: bool,
    pub is_rest: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PortableObjectMember {
    Property {
        name: String,
        ty: PortableType,
        optional: bool,
        readonly: bool,
    },
    Method {
        name: String,
        params: Vec<PortableParam>,
        return_type: PortableType,
        optional: bool,
        is_arrow: bool,
    },
    Index {
        param_name: String,
        key_ty: PortableType,
        value_ty: PortableType,
    },
    Callable {
        params: Vec<PortableParam>,
        return_type: PortableType,
        is_arrow: bool,
    },
}
