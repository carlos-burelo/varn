use varn_core::{Atom, LangPrimitive, TypeKind};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct CheckerTyId(pub(super) u128);

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct TyListId(pub(super) u128);

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct FunctionTypeId(pub(super) u128);

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct ObjectMembersId(pub(super) u128);

pub type InternedTypeKind =
    TypeKind<CheckerTyId, Atom, TyListId, FunctionTypeId, ObjectMembersId, ()>;

impl CheckerTyId {
    pub const INT: CheckerTyId = CheckerTyId(0);
    pub const FLOAT: CheckerTyId = CheckerTyId(1);
    pub const DECIMAL: CheckerTyId = CheckerTyId(2);
    pub const BIGINT: CheckerTyId = CheckerTyId(3);
    pub const STR: CheckerTyId = CheckerTyId(4);
    pub const CHAR: CheckerTyId = CheckerTyId(5);
    pub const BOOL: CheckerTyId = CheckerTyId(6);
    pub const VOID: CheckerTyId = CheckerTyId(7);
    pub const NULL: CheckerTyId = CheckerTyId(8);
    pub const NEVER: CheckerTyId = CheckerTyId(9);
    pub const DYNAMIC: CheckerTyId = CheckerTyId(10);
    pub const THIS: CheckerTyId = CheckerTyId(11);
}

pub(super) fn seeded_id(kind: &InternedTypeKind) -> Option<CheckerTyId> {
    match kind {
        TypeKind::Primitive(p) => Some(match p {
            LangPrimitive::Int => CheckerTyId::INT,
            LangPrimitive::Float => CheckerTyId::FLOAT,
            LangPrimitive::Decimal => CheckerTyId::DECIMAL,
            LangPrimitive::BigInt => CheckerTyId::BIGINT,
            LangPrimitive::Str => CheckerTyId::STR,
            LangPrimitive::Char => CheckerTyId::CHAR,
            LangPrimitive::Bool => CheckerTyId::BOOL,
            LangPrimitive::Void => CheckerTyId::VOID,
            LangPrimitive::Null => CheckerTyId::NULL,
            LangPrimitive::Never => CheckerTyId::NEVER,
            LangPrimitive::Dynamic => CheckerTyId::DYNAMIC,
        }),
        TypeKind::This => Some(CheckerTyId::THIS),
        TypeKind::Builtin(_)
        | TypeKind::Literal(_)
        | TypeKind::Array(_)
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::Named(..)
        | TypeKind::Generic(..)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Object(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => None,
    }
}
