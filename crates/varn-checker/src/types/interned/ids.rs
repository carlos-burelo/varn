use varn_core::{Atom, LangPrimitive, TypeKind};

/// Content-addressed id of a shape. `Copy`, so cloning a `Type` is trivial and
/// comparing two types is comparing two `u128`.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct CheckerTyId(pub(super) u128);

/// Content hash of a `Vec<CheckerTyId>` (union/tuple/intersection members,
/// generic args, ...).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct TyListId(pub(super) u128);

/// Content hash of a `FunctionType`.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct FunctionTypeId(pub(super) u128);

/// Content hash of a `Vec<ObjectTypeMember>`.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct ObjectMembersId(pub(super) u128);

/// Interned form of `checker::types::SemanticTypeKind`. Same variants as
/// `varn_core::TypeKind`, parameters substituted per the module doc above.
/// `Copy` because every substituted parameter is `Copy`.
pub type InternedTypeKind =
    TypeKind<CheckerTyId, Atom, TyListId, FunctionTypeId, ObjectMembersId, ()>;

/// Fixed ids for the ~13 zero-argument/intrinsic shapes every checker session
/// needs (the `Type::Int`/`Type::Str`/... constants `type_impl.rs` exposes).
/// Unlike the rest of the table, these are NOT content hashes: they are small
/// constants so `Type::INT` can be a `const`. `CheckerTyTable::new` seeds
/// EXACTLY these, and `intern` special-cases them back to the same ids.
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

/// `CheckerTyId` of the seeded intrinsic `tag`, or `None` for the tags that
/// have no reserved id (e.g. `Bytes`): those are content-addressed like any
/// other shape.
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
        _ => None,
    }
}
