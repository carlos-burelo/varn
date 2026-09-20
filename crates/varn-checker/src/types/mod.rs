mod async_fn;
mod class_member_impl;
mod context;
mod display;
pub mod interned;
mod object_member_impl;
mod portable;
mod type_impl;

pub use async_fn::{async_fn_return, awaited, generator_of, is_awaitable};
pub use interned::{
    CheckerTyId, CheckerTyTable, FunctionTypeId, InternedTypeKind, ObjectMembersId, TyListId,
};
pub use portable::{
    decode as decode_portable_type, encode as encode_portable_type, PortableFunction,
    PortableObjectMember, PortableParam, PortableType,
};

use rustc_hash::FxHashMap;
use std::fmt;
use std::rc::Rc;
use varn_core::ast::operators::Visibility;
use varn_core::{TypeKind, TypeTag};

/// The checker's public type handle. Used to be a recursive, heap-allocated
/// `Type(SemanticTypeKind, bool)` — every distinct shape (`Array<int>` at ten
/// call sites, say) was its own boxed tree, cloned on every pass. Fase 1
/// Componente 3 replaces the payload with a hash-consed `CheckerTyId`: the
/// SAME shape always gets the SAME id (see `CheckerTyTable::intern`), so
/// comparing two types is an integer compare and cloning a `Type` is a
/// `Copy`. The `bool` survives unchanged — it is NOT part of a type's
/// hash-consed identity (two occurrences of the same shape can be tainted
/// independently, e.g. one narrowed-from-dynamic call result and one
/// statically-known local of the same type), so it stays outside `CheckerTyId`
/// and rides along on this wrapper instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Type(pub CheckerTyId, pub bool);

impl Type {
    pub fn id(&self) -> CheckerTyId {
        self.0
    }

    pub fn kind(&self, table: &CheckerTyTable) -> InternedTypeKind {
        table.get(self.0)
    }

    pub fn tainted(mut self) -> Self {
        self.1 = true;
        self
    }
}

impl Default for Type {
    fn default() -> Self {
        Type(CheckerTyId::DYNAMIC, false)
    }
}

/// `FunctionParam`/`FunctionType`/`ObjectTypeMember` recursive type fields
/// (`ty`, `return_type`, `key_ty`, `value_ty`) reference `CheckerTyId`
/// directly rather than `Type` — the `tainted` bool is a property of a
/// specific expression occurrence, not of a function signature or object
/// shape's declared members, so it has nothing to attach to here (matches
/// the plan's Task 21 Step 2 example). `name` fields stay `Rc<str>` rather
/// than migrating to `Atom`: the plan's interned-form docs only mandate
/// `Atom` for `TypeKind::Named`/`Generic`'s own name slot (already done in
/// Task 19/20's `InternedTypeKind`), and threading an interner through every
/// `FunctionParam`/`ObjectTypeMember` construction site for no dedup benefit
/// `Rc<str>` doesn't already give is scope this task doesn't need — documented
/// deviation from the plan's illustrative (not prescriptive) `Option<Atom>`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FunctionParam {
    pub name: Option<Rc<str>>,
    pub ty: CheckerTyId,
    pub optional: bool,
    pub is_rest: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FunctionType {
    pub params: Vec<FunctionParam>,
    pub return_type: CheckerTyId,
    pub is_arrow: bool,
    pub type_params: Vec<Rc<str>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ObjectTypeMember {
    Property {
        name: Rc<str>,
        ty: CheckerTyId,
        optional: bool,
        readonly: bool,
    },
    Method {
        name: Rc<str>,
        params: Vec<FunctionParam>,
        return_type: CheckerTyId,
        optional: bool,
        is_arrow: bool,
    },
    Index {
        param_name: Rc<str>,
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
