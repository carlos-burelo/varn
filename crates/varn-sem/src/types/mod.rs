mod async_fn;
mod class_member_impl;
mod context;
mod core_sum;
mod display;
pub mod interned;
pub mod numeric_literal;
mod object_member_impl;
mod type_algebra;
mod type_impl;

pub use async_fn::{async_fn_return, awaited, generator_of, is_awaitable};
pub use interned::{
    CheckerTyId, CheckerTyTable, FunctionTypeId, InternedTypeKind, ObjectMembersId, TyListId,
    TySlice,
};

use rustc_hash::FxHashMap;
use std::fmt;
use std::sync::Arc;
use varn_core::ast::operators::Visibility;
use varn_core::TypeKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Origin {
    Resolved,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Type(pub CheckerTyId, pub Origin);

impl Type {
    pub const fn resolved(id: CheckerTyId) -> Self {
        Type(id, Origin::Resolved)
    }

    pub fn id(&self) -> CheckerTyId {
        self.0
    }

    pub fn kind(&self, table: &CheckerTyTable) -> InternedTypeKind {
        table.get(self.0)
    }

    pub fn is_error(&self) -> bool {
        self.1 == Origin::Error
    }
}

impl Default for Type {
    fn default() -> Self {
        Type::resolved(CheckerTyId::DYNAMIC)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FunctionParam {
    pub name: Option<Arc<str>>,
    pub ty: CheckerTyId,
    pub optional: bool,
    pub is_rest: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FunctionType {
    pub params: Vec<FunctionParam>,
    pub return_type: CheckerTyId,
    pub is_arrow: bool,
    pub type_params: Vec<Arc<str>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ObjectTypeMember {
    Property {
        name: Arc<str>,
        ty: CheckerTyId,
        optional: bool,
        readonly: bool,
    },
    Method {
        name: Arc<str>,
        params: Vec<FunctionParam>,
        return_type: CheckerTyId,
        optional: bool,
        is_arrow: bool,
    },
    Index {
        param_name: Arc<str>,
        key_ty: CheckerTyId,
        value_ty: CheckerTyId,
    },
    Callable {
        params: Vec<FunctionParam>,
        return_type: CheckerTyId,
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
    pub name: Arc<str>,
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
    pub is_builtin_or_intrinsic: bool,
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

    pub fn ty(&self) -> CheckerTyId {
        match self {
            ObjectTypeMember::Property { ty, .. } => *ty,
            ObjectTypeMember::Method { .. } => CheckerTyId::DYNAMIC,
            ObjectTypeMember::Index { value_ty, .. } => *value_ty,
            ObjectTypeMember::Callable { return_type, .. } => *return_type,
        }
    }
}
