mod class_member_impl;
mod context;
mod display;
mod object_member_impl;
mod type_impl;

use rustc_hash::FxHashMap;
use std::fmt;
use std::rc::Rc;
use varn_core::ast::operators::Visibility;
use varn_core::{TypeKind, TypeTag};

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FunctionParam {
    pub name: Option<Rc<str>>,
    pub ty: Type,
    pub optional: bool,
    pub is_rest: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FunctionType {
    pub params: Vec<FunctionParam>,
    pub return_type: Box<Type>,
    pub is_arrow: bool,
    pub type_params: Vec<Rc<str>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Type(
    pub TypeKind<Box<Type>, Rc<str>, Vec<Type>, FunctionType, Vec<ObjectTypeMember>, ()>,
    pub bool,
);

impl Type {
    pub fn kind(
        &self,
    ) -> &TypeKind<Box<Type>, Rc<str>, Vec<Type>, FunctionType, Vec<ObjectTypeMember>, ()> {
        &self.0
    }

    pub fn tainted(mut self) -> Self {
        self.1 = true;
        self
    }

}

impl Default for Type {
    fn default() -> Self {
        Type(TypeKind::Intrinsic(TypeTag::Dynamic), false)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ObjectTypeMember {
    Property {
        name: Rc<str>,
        ty: Type,
        optional: bool,
        readonly: bool,
    },
    Method {
        name: Rc<str>,
        params: Vec<FunctionParam>,
        return_type: Box<Type>,
        optional: bool,
        is_arrow: bool,
    },
    Index {
        param_name: Rc<str>,
        key_ty: Box<Type>,
        value_ty: Box<Type>,
    },
    Callable {
        params: Vec<FunctionParam>,
        return_type: Box<Type>,
        is_arrow: bool,
    },
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Default, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ClassMemberKind {
    Constructor,
    Method,
    Function,
    #[default]
    Property,
    Variable,
    Getter,
    Setter,
    Class,
    Interface,
    Namespace,
    Enum,
    Struct,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ClassMemberInfo {
    pub name: Rc<str>,
    pub kind: ClassMemberKind,
    pub is_async: bool,
    pub is_generator: bool,
    pub is_static: bool,
    pub is_optional: bool,
    pub line: u32,
    pub col: u32,
    pub offset: u32,
    pub ty: Type,
    pub members: Vec<ClassMemberInfo>,
    pub visibility: Option<Visibility>,
    pub is_abstract: bool,
    pub is_readonly: bool,
    pub is_override: bool,
    pub symbol_id: Option<usize>,
}

pub use context::TypeContext;

impl ObjectTypeMember {
    pub fn name(&self) -> &str {
        match self {
            ObjectTypeMember::Property { name, .. } => name.as_ref(),
            ObjectTypeMember::Method { name, .. } => name.as_ref(),
            ObjectTypeMember::Index { param_name, .. } => param_name.as_ref(),
            ObjectTypeMember::Callable { .. } => "",
        }
    }

    pub fn ty(&self) -> &Type {
        match self {
            ObjectTypeMember::Property { ty, .. } => ty,
            ObjectTypeMember::Method { .. } => &Type::Dynamic,
            ObjectTypeMember::Index { value_ty, .. } => value_ty,
            ObjectTypeMember::Callable { return_type, .. } => return_type,
        }
    }
}
