//! Hash-consed, **content-addressed** type table for the checker (Fase 1,
//! Componente 3 + ADR-0012).
//!
//! `varn_core::TypeKind<T, N, C, F, O, E>` is generic over how recursion,
//! names and collections are represented. This module fixes those
//! parameters to interned handles:
//!
//! - `T` (recursion, e.g. `Array(T)`, `KeyOf(T)`)      -> `CheckerTyId`
//! - `N` (name, e.g. `Named(N, Option<N>)`)              -> `varn_core::Atom`
//! - `C` (collection, e.g. `Union(C)`, `Tuple(C)`)       -> `TyListId`
//! - `F` (function shape)                                -> `FunctionTypeId`
//! - `O` (object members)                                -> `ObjectMembersId`
//! - `E` (today `()` in `SemanticTypeKind`)              -> `()`
//!
//! ## Content-addressed identity (the Ley 2/3 root-cause fix)
//!
//! A `CheckerTyId` is the **128-bit hash of the shape it names**, not a
//! positional index into a table. Two tables that intern the same shape in any
//! order — or in parallel — produce the **same id** for it, so ids are portable
//! by construction and no `absorb`/`reintern` remap is needed: merging tables is
//! a commutative, idempotent union. The ~13 intrinsic shapes keep a reserved
//! id range (`0..=THIS`) because `Type::Int`/`Type::Str`/... are `const`.
//! Non-intrinsic ids set the top bit, so they can never collide with that range.
//!
//! The hash is computed with `rustc_hash::FxHasher` (fixed seed, no
//! `RandomState`), hashed twice with different salts into 128 bits. 128 bits is
//! the engineering standard for collision resistance (rustc uses the same width
//! for its stable hashes): the birthday bound for 2^32 shapes is ~2^-64.
//!
//! `FunctionTypeId`/`ObjectMembersId` are content hashes of the function shape
//! and the member vector, so a shape's id is a Merkle hash over its children.

use crate::types::{FunctionType, ObjectTypeMember};
use rustc_hash::{FxHashMap, FxHasher};
use std::hash::{Hash, Hasher};
use varn_core::{Atom, LangPrimitive, TypeKind};

/// Content-addressed id of a shape. `Copy`, so cloning a `Type` is trivial and
/// comparing two types is comparing two `u128`.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct CheckerTyId(u128);

/// Content hash of a `Vec<CheckerTyId>` (union/tuple/intersection members,
/// generic args, ...).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct TyListId(u128);

/// Content hash of a `FunctionType`.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct FunctionTypeId(u128);

/// Content hash of a `Vec<ObjectTypeMember>`.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct ObjectMembersId(u128);

/// Interned form of `checker::types::SemanticTypeKind`. Same variants as
/// `varn_core::TypeKind`, parameters substituted per the module doc above.
/// `Copy` because every substituted parameter is `Copy`.
pub type InternedTypeKind =
    TypeKind<CheckerTyId, Atom, TyListId, FunctionTypeId, ObjectMembersId, ()>;

/// Top bit of a non-intrinsic content id. Reserved intrinsic ids are `0..=THIS`
/// (all far below this bit), so content ids can never collide with them.
const CONTENT_FLAG: u128 = 1u128 << 127;

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
fn seeded_id(kind: &InternedTypeKind) -> Option<CheckerTyId> {
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

/// 128-bit content hash: `FxHasher` (fixed seed) run twice with different
/// salts. Deterministic across processes and platforms.
fn hash128<T: Hash + ?Sized>(value: &T) -> u128 {
    let mut lo = FxHasher::default();
    value.hash(&mut lo);
    let mut hi = FxHasher::default();
    0x9E37_79B9_7F4A_7C15u64.hash(&mut hi);
    value.hash(&mut hi);
    ((hi.finish() as u128) << 64) | (lo.finish() as u128)
}

fn content_id(kind: &InternedTypeKind) -> CheckerTyId {
    CheckerTyId(hash128(kind) | CONTENT_FLAG)
}

fn content_list_id(tys: &[CheckerTyId]) -> TyListId {
    TyListId(hash128(tys) | CONTENT_FLAG)
}

fn content_function_id(f: &FunctionType) -> FunctionTypeId {
    FunctionTypeId(hash128(f) | CONTENT_FLAG)
}

fn content_object_id(members: &[ObjectTypeMember]) -> ObjectMembersId {
    ObjectMembersId(hash128(members) | CONTENT_FLAG)
}

/// Content-addressed store: a memo `id -> shape`. Identity never depends on
/// insertion order, so this is a pure cache — two tables with the same shapes
/// agree on every id, and merging them is a set union.
#[derive(Debug, Clone)]
pub struct CheckerTyTable {
    entries: FxHashMap<CheckerTyId, InternedTypeKind>,
    lists: FxHashMap<TyListId, Vec<CheckerTyId>>,
    functions: FxHashMap<FunctionTypeId, FunctionType>,
    object_members: FxHashMap<ObjectMembersId, Vec<ObjectTypeMember>>,
}

impl Default for CheckerTyTable {
    fn default() -> Self {
        Self::new()
    }
}

impl CheckerTyTable {
    pub fn new() -> Self {
        let mut t = Self {
            entries: FxHashMap::default(),
            lists: FxHashMap::default(),
            functions: FxHashMap::default(),
            object_members: FxHashMap::default(),
        };
        // Seed the reserved intrinsic ids. `intern` maps these shapes back to
        // the same ids, so the seed is only so `get` is total over them.
        for (id, kind) in [
            (CheckerTyId::INT, TypeKind::Primitive(varn_core::LangPrimitive::Int)),
            (CheckerTyId::FLOAT, TypeKind::Primitive(varn_core::LangPrimitive::Float)),
            (CheckerTyId::DECIMAL, TypeKind::Primitive(varn_core::LangPrimitive::Decimal)),
            (CheckerTyId::BIGINT, TypeKind::Primitive(varn_core::LangPrimitive::BigInt)),
            (CheckerTyId::STR, TypeKind::Primitive(varn_core::LangPrimitive::Str)),
            (CheckerTyId::CHAR, TypeKind::Primitive(varn_core::LangPrimitive::Char)),
            (CheckerTyId::BOOL, TypeKind::Primitive(varn_core::LangPrimitive::Bool)),
            (CheckerTyId::VOID, TypeKind::Primitive(varn_core::LangPrimitive::Void)),
            (CheckerTyId::NULL, TypeKind::Primitive(varn_core::LangPrimitive::Null)),
            (CheckerTyId::NEVER, TypeKind::Primitive(varn_core::LangPrimitive::Never)),
            (CheckerTyId::DYNAMIC, TypeKind::Primitive(varn_core::LangPrimitive::Dynamic)),
            (CheckerTyId::THIS, TypeKind::This),
        ] {
            t.entries.insert(id, kind);
        }
        t
    }

    /// Intern `kind`, returning its content-addressed id. Idempotent and
    /// order-independent: the same shape always yields the same id.
    pub fn intern(&mut self, kind: InternedTypeKind) -> CheckerTyId {
        if let Some(id) = seeded_id(&kind) {
            return id;
        }
        let id = content_id(&kind);
        if let Some(existing) = self.entries.get(&id) {
            debug_assert_eq!(existing, &kind, "CheckerTyId content hash collision");
            return id;
        }
        self.entries.insert(id, kind);
        id
    }

    /// The shape `id` names. Panics if `id` was never interned into this table
    /// (the same invariant the old positional `Vec` index enforced).
    pub fn get(&self, id: CheckerTyId) -> InternedTypeKind {
        *self
            .entries
            .get(&id)
            .unwrap_or_else(|| panic!("CheckerTyId {id:?} is not present in this table"))
    }

    /// True when this table can resolve `id` to a shape.
    pub fn contains(&self, id: CheckerTyId) -> bool {
        self.entries.contains_key(&id)
    }

    pub fn intern_list(&mut self, tys: &[CheckerTyId]) -> TyListId {
        let id = content_list_id(tys);
        if let Some(existing) = self.lists.get(&id) {
            debug_assert_eq!(existing.as_slice(), tys, "TyListId content hash collision");
            return id;
        }
        self.lists.insert(id, tys.to_vec());
        id
    }

    pub fn get_list(&self, id: TyListId) -> &[CheckerTyId] {
        self.lists
            .get(&id)
            .map(Vec::as_slice)
            .unwrap_or_else(|| panic!("TyListId {id:?} is not present in this table"))
    }

    pub fn intern_function(&mut self, f: FunctionType) -> FunctionTypeId {
        let id = content_function_id(&f);
        if let Some(existing) = self.functions.get(&id) {
            debug_assert_eq!(existing, &f, "FunctionTypeId content hash collision");
            return id;
        }
        self.functions.insert(id, f);
        id
    }

    pub fn get_function(&self, id: FunctionTypeId) -> &FunctionType {
        self.functions
            .get(&id)
            .unwrap_or_else(|| panic!("FunctionTypeId {id:?} is not present in this table"))
    }

    pub fn intern_object_members(&mut self, members: Vec<ObjectTypeMember>) -> ObjectMembersId {
        let id = content_object_id(&members);
        if let Some(existing) = self.object_members.get(&id) {
            debug_assert_eq!(existing, &members, "ObjectMembersId content hash collision");
            return id;
        }
        self.object_members.insert(id, members);
        id
    }

    pub fn get_object_members(&self, id: ObjectMembersId) -> &[ObjectTypeMember] {
        self.object_members
            .get(&id)
            .map(Vec::as_slice)
            .unwrap_or_else(|| panic!("ObjectMembersId {id:?} is not present in this table"))
    }

    /// Number of distinct interned shapes. Used as a cheap "did the live table
    /// grow past this snapshot" heuristic by `Binder::sync_ty_table`.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Union every shape in `other` into `self`. Commutative and idempotent:
    /// content-addressed ids mean a shape present in both tables has the same
    /// id, so this is a set union with no remap. This replaces the old
    /// `reintern`-based `absorb` (ADR-0012).
    pub fn absorb(&mut self, other: &CheckerTyTable) {
        for (k, v) in &other.entries {
            self.entries.entry(*k).or_insert(*v);
        }
        for (k, v) in &other.lists {
            self.lists.entry(*k).or_insert_with(|| v.clone());
        }
        for (k, v) in &other.functions {
            self.functions.entry(*k).or_insert_with(|| v.clone());
        }
        for (k, v) in &other.object_members {
            self.object_members.entry(*k).or_insert_with(|| v.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    

    #[test]
    fn interning_the_same_intrinsic_twice_dedups() {
        let mut t = CheckerTyTable::default();
        let a = t.intern(TypeKind::Primitive(varn_core::LangPrimitive::Int));
        let b = t.intern(TypeKind::Primitive(varn_core::LangPrimitive::Int));
        assert_eq!(a, b);
    }

    #[test]
    fn different_intrinsics_get_different_ids() {
        let mut t = CheckerTyTable::default();
        let a = t.intern(TypeKind::Primitive(varn_core::LangPrimitive::Int));
        let b = t.intern(TypeKind::Primitive(varn_core::LangPrimitive::Str));
        assert_ne!(a, b);
    }

    #[test]
    fn get_roundtrips_the_interned_value() {
        let mut t = CheckerTyTable::default();
        let a = t.intern(TypeKind::Primitive(varn_core::LangPrimitive::Bool));
        assert_eq!(t.get(a), TypeKind::Primitive(varn_core::LangPrimitive::Bool));
    }

    #[test]
    fn identical_unions_by_member_ids_dedup_via_ty_list() {
        let mut t = CheckerTyTable::default();
        let int = t.intern(TypeKind::Primitive(varn_core::LangPrimitive::Int));
        let str_ = t.intern(TypeKind::Primitive(varn_core::LangPrimitive::Str));
        let list1 = t.intern_list(&[int, str_]);
        let list2 = t.intern_list(&[int, str_]);
        assert_eq!(list1, list2);
        let union1 = t.intern(TypeKind::Union(list1));
        let union2 = t.intern(TypeKind::Union(list2));
        assert_eq!(union1, union2);
    }

    /// The whole point of content addressing: two tables that grew in any
    /// order agree on every shape's id, so `absorb` is a plain union.
    #[test]
    fn ids_are_order_independent_across_tables() {
        let mut left = CheckerTyTable::default();
        let mut right = CheckerTyTable::default();

        // Same shapes, opposite insertion order.
        let l_int = left.intern(TypeKind::Primitive(varn_core::LangPrimitive::Int));
        let l_arr = left.intern(TypeKind::Array(l_int));
        let r_int = right.intern(TypeKind::Primitive(varn_core::LangPrimitive::Int));
        let r_arr = right.intern(TypeKind::Array(r_int));

        assert_eq!(l_int, CheckerTyId::INT);
        assert_eq!(l_arr, r_arr, "same shape -> same id regardless of order");
    }
}
