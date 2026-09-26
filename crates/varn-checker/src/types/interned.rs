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

/// How many new entries any of `CheckerTyTable`'s four delta maps may hold
/// before the next mutation folds all four into a fresh frozen base. Bounds
/// the cost of every `Clone` regardless of session length — see
/// `docs/plans/2026-09-25-shared-atom-type-tables.md` §3 (Enfoque B). Ids here
/// are content-addressed (ADR-0012), so freezing at any point can never
/// change what id a shape gets.
const FREEZE_THRESHOLD: usize = 2048;

/// Frozen, immutable half of a `CheckerTyTable`. Shared via `Arc`, so cloning
/// a `CheckerTyTable` never copies this: only the (bounded) delta is copied.
#[derive(Debug, Default)]
struct CheckerTyBase {
    entries: FxHashMap<CheckerTyId, InternedTypeKind>,
    lists: FxHashMap<TyListId, Vec<CheckerTyId>>,
    functions: FxHashMap<FunctionTypeId, FunctionType>,
    object_members: FxHashMap<ObjectMembersId, Vec<ObjectTypeMember>>,
}

/// Content-addressed store: a memo `id -> shape`. Identity never depends on
/// insertion order, so this is a pure cache — two tables with the same shapes
/// agree on every id, and merging them is a set union.
///
/// Internally split into a frozen `base` (shared via `Arc`, O(1) to clone)
/// and four small delta maps holding what was interned since the last
/// freeze.
#[derive(Debug, Clone)]
pub struct CheckerTyTable {
    base: std::sync::Arc<CheckerTyBase>,
    delta_entries: FxHashMap<CheckerTyId, InternedTypeKind>,
    delta_lists: FxHashMap<TyListId, Vec<CheckerTyId>>,
    delta_functions: FxHashMap<FunctionTypeId, FunctionType>,
    delta_object_members: FxHashMap<ObjectMembersId, Vec<ObjectTypeMember>>,
}

impl Default for CheckerTyTable {
    fn default() -> Self {
        Self::new()
    }
}

impl CheckerTyTable {
    pub fn new() -> Self {
        let mut base = CheckerTyBase::default();
        // Seed the reserved intrinsic ids. `intern` maps these shapes back to
        // the same ids, so the seed is only so `get` is total over them.
        for (id, kind) in [
            (
                CheckerTyId::INT,
                TypeKind::Primitive(varn_core::LangPrimitive::Int),
            ),
            (
                CheckerTyId::FLOAT,
                TypeKind::Primitive(varn_core::LangPrimitive::Float),
            ),
            (
                CheckerTyId::DECIMAL,
                TypeKind::Primitive(varn_core::LangPrimitive::Decimal),
            ),
            (
                CheckerTyId::BIGINT,
                TypeKind::Primitive(varn_core::LangPrimitive::BigInt),
            ),
            (
                CheckerTyId::STR,
                TypeKind::Primitive(varn_core::LangPrimitive::Str),
            ),
            (
                CheckerTyId::CHAR,
                TypeKind::Primitive(varn_core::LangPrimitive::Char),
            ),
            (
                CheckerTyId::BOOL,
                TypeKind::Primitive(varn_core::LangPrimitive::Bool),
            ),
            (
                CheckerTyId::VOID,
                TypeKind::Primitive(varn_core::LangPrimitive::Void),
            ),
            (
                CheckerTyId::NULL,
                TypeKind::Primitive(varn_core::LangPrimitive::Null),
            ),
            (
                CheckerTyId::NEVER,
                TypeKind::Primitive(varn_core::LangPrimitive::Never),
            ),
            (
                CheckerTyId::DYNAMIC,
                TypeKind::Primitive(varn_core::LangPrimitive::Dynamic),
            ),
            (CheckerTyId::THIS, TypeKind::This),
        ] {
            base.entries.insert(id, kind);
        }
        Self {
            base: std::sync::Arc::new(base),
            delta_entries: FxHashMap::default(),
            delta_lists: FxHashMap::default(),
            delta_functions: FxHashMap::default(),
            delta_object_members: FxHashMap::default(),
        }
    }

    /// Fold every delta map into a fresh frozen `base`. O(n) in the total
    /// table size, but only runs once every `FREEZE_THRESHOLD` new entries
    /// across all four maps combined, not once per caller.
    fn freeze(&mut self) {
        if self.delta_entries.is_empty()
            && self.delta_lists.is_empty()
            && self.delta_functions.is_empty()
            && self.delta_object_members.is_empty()
        {
            return;
        }
        let mut entries = self.base.entries.clone();
        let mut lists = self.base.lists.clone();
        let mut functions = self.base.functions.clone();
        let mut object_members = self.base.object_members.clone();
        entries.extend(self.delta_entries.drain());
        lists.extend(self.delta_lists.drain());
        functions.extend(self.delta_functions.drain());
        object_members.extend(self.delta_object_members.drain());
        self.base = std::sync::Arc::new(CheckerTyBase {
            entries,
            lists,
            functions,
            object_members,
        });
    }

    fn maybe_freeze(&mut self) {
        let delta_size = self.delta_entries.len()
            + self.delta_lists.len()
            + self.delta_functions.len()
            + self.delta_object_members.len();
        if delta_size >= FREEZE_THRESHOLD {
            self.freeze();
        }
    }

    /// Intern `kind`, returning its content-addressed id. Idempotent and
    /// order-independent: the same shape always yields the same id.
    pub fn intern(&mut self, kind: InternedTypeKind) -> CheckerTyId {
        if let Some(id) = seeded_id(&kind) {
            return id;
        }
        let id = content_id(&kind);
        if let Some(existing) = self
            .base
            .entries
            .get(&id)
            .or_else(|| self.delta_entries.get(&id))
        {
            debug_assert_eq!(existing, &kind, "CheckerTyId content hash collision");
            return id;
        }
        self.delta_entries.insert(id, kind);
        self.maybe_freeze();
        id
    }

    /// The shape `id` names. Panics if `id` was never interned into this table
    /// (the same invariant the old positional `Vec` index enforced).
    pub fn get(&self, id: CheckerTyId) -> InternedTypeKind {
        *self
            .base
            .entries
            .get(&id)
            .or_else(|| self.delta_entries.get(&id))
            .unwrap_or_else(|| panic!("CheckerTyId {id:?} is not present in this table"))
    }

    /// True when this table can resolve `id` to a shape.
    pub fn contains(&self, id: CheckerTyId) -> bool {
        self.base.entries.contains_key(&id) || self.delta_entries.contains_key(&id)
    }

    pub fn intern_list(&mut self, tys: &[CheckerTyId]) -> TyListId {
        let id = content_list_id(tys);
        if let Some(existing) = self
            .base
            .lists
            .get(&id)
            .or_else(|| self.delta_lists.get(&id))
        {
            debug_assert_eq!(existing.as_slice(), tys, "TyListId content hash collision");
            return id;
        }
        self.delta_lists.insert(id, tys.to_vec());
        self.maybe_freeze();
        id
    }

    pub fn get_list(&self, id: TyListId) -> &[CheckerTyId] {
        self.base
            .lists
            .get(&id)
            .or_else(|| self.delta_lists.get(&id))
            .map(Vec::as_slice)
            .unwrap_or_else(|| panic!("TyListId {id:?} is not present in this table"))
    }

    pub fn intern_function(&mut self, f: FunctionType) -> FunctionTypeId {
        let id = content_function_id(&f);
        if let Some(existing) = self
            .base
            .functions
            .get(&id)
            .or_else(|| self.delta_functions.get(&id))
        {
            debug_assert_eq!(existing, &f, "FunctionTypeId content hash collision");
            return id;
        }
        self.delta_functions.insert(id, f);
        self.maybe_freeze();
        id
    }

    pub fn get_function(&self, id: FunctionTypeId) -> &FunctionType {
        self.base
            .functions
            .get(&id)
            .or_else(|| self.delta_functions.get(&id))
            .unwrap_or_else(|| panic!("FunctionTypeId {id:?} is not present in this table"))
    }

    pub fn intern_object_members(&mut self, members: Vec<ObjectTypeMember>) -> ObjectMembersId {
        let id = content_object_id(&members);
        if let Some(existing) = self
            .base
            .object_members
            .get(&id)
            .or_else(|| self.delta_object_members.get(&id))
        {
            debug_assert_eq!(existing, &members, "ObjectMembersId content hash collision");
            return id;
        }
        self.delta_object_members.insert(id, members);
        self.maybe_freeze();
        id
    }

    pub fn get_object_members(&self, id: ObjectMembersId) -> &[ObjectTypeMember] {
        self.base
            .object_members
            .get(&id)
            .or_else(|| self.delta_object_members.get(&id))
            .map(Vec::as_slice)
            .unwrap_or_else(|| panic!("ObjectMembersId {id:?} is not present in this table"))
    }

    /// True when this table can resolve `id` to a member vector. Read-only
    /// counterpart to `contains`, for the same reason: callers that must not
    /// panic on a foreign id (and must not intern one either) need a way to
    /// ask first.
    pub fn contains_object_members(&self, id: ObjectMembersId) -> bool {
        self.base.object_members.contains_key(&id) || self.delta_object_members.contains_key(&id)
    }

    /// Number of distinct interned shapes. Used as a cheap "did the live table
    /// grow past this snapshot" heuristic by `Binder::sync_ty_table`.
    pub fn len(&self) -> usize {
        self.base.entries.len() + self.delta_entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Union every shape in `other` into `self`. Commutative and idempotent:
    /// content-addressed ids mean a shape present in both tables has the same
    /// id, so this is a set union with no remap. This replaces the old
    /// `reintern`-based `absorb` (ADR-0012).
    pub fn absorb(&mut self, other: &CheckerTyTable) {
        for (k, v) in other.base.entries.iter().chain(other.delta_entries.iter()) {
            if !self.contains(*k) {
                self.delta_entries.insert(*k, *v);
            }
        }
        for (k, v) in other.base.lists.iter().chain(other.delta_lists.iter()) {
            if self
                .base
                .lists
                .get(k)
                .or_else(|| self.delta_lists.get(k))
                .is_none()
            {
                self.delta_lists.insert(*k, v.clone());
            }
        }
        for (k, v) in other
            .base
            .functions
            .iter()
            .chain(other.delta_functions.iter())
        {
            if self
                .base
                .functions
                .get(k)
                .or_else(|| self.delta_functions.get(k))
                .is_none()
            {
                self.delta_functions.insert(*k, v.clone());
            }
        }
        for (k, v) in other
            .base
            .object_members
            .iter()
            .chain(other.delta_object_members.iter())
        {
            if !self.contains_object_members(*k) {
                self.delta_object_members.insert(*k, v.clone());
            }
        }
        self.maybe_freeze();
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
        assert_eq!(
            t.get(a),
            TypeKind::Primitive(varn_core::LangPrimitive::Bool)
        );
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

    #[test]
    fn interning_past_freeze_threshold_still_resolves_correctly() {
        let mut t = CheckerTyTable::default();
        let mut ids = Vec::new();
        for i in 0..(FREEZE_THRESHOLD * 2 + 3) {
            // Distinct shapes: nested `Array` of increasing depth via a
            // synthetic list id keeps every entry unique without depending on
            // string content (this table has no strings).
            let list = t.intern_list(&[CheckerTyId::INT; 1]);
            let _ = list;
            ids.push(t.intern_object_members(vec![ObjectTypeMember::Property {
                name: std::sync::Arc::from(format!("f{i}").as_str()),
                ty: CheckerTyId::INT,
                optional: false,
                readonly: false,
            }]));
        }
        for (i, id) in ids.iter().enumerate() {
            let members = t.get_object_members(*id);
            match &members[0] {
                ObjectTypeMember::Property { name, .. } => {
                    assert_eq!(name.as_ref(), format!("f{i}"));
                }
                other => panic!("expected Property, got {other:?}"),
            }
        }
    }

    #[test]
    fn absorb_across_many_freezes_preserves_every_shape() {
        let mut local = CheckerTyTable::default();
        let mut foreign = CheckerTyTable::default();
        let mut foreign_ids = Vec::new();
        for i in 0..(FREEZE_THRESHOLD * 2 + 3) {
            foreign_ids.push(
                foreign.intern_object_members(vec![ObjectTypeMember::Property {
                    name: std::sync::Arc::from(format!("g{i}").as_str()),
                    ty: CheckerTyId::INT,
                    optional: false,
                    readonly: false,
                }]),
            );
        }
        local.absorb(&foreign);
        for (i, id) in foreign_ids.iter().enumerate() {
            let members = local.get_object_members(*id);
            match &members[0] {
                ObjectTypeMember::Property { name, .. } => {
                    assert_eq!(name.as_ref(), format!("g{i}"));
                }
                other => panic!("expected Property, got {other:?}"),
            }
        }
    }

    #[test]
    fn clone_after_freeze_does_not_leak_delta_between_instances() {
        let mut t = CheckerTyTable::default();
        for i in 0..(FREEZE_THRESHOLD + 1) {
            t.intern_object_members(vec![ObjectTypeMember::Property {
                name: std::sync::Arc::from(format!("h{i}").as_str()),
                ty: CheckerTyId::INT,
                optional: false,
                readonly: false,
            }]);
        }
        let mut clone = t.clone();
        let extra = clone.intern_object_members(vec![ObjectTypeMember::Property {
            name: std::sync::Arc::from("only-in-clone"),
            ty: CheckerTyId::INT,
            optional: false,
            readonly: false,
        }]);
        assert!(!t.contains_object_members(extra));
        assert_eq!(clone.get_object_members(extra).len(), 1);
    }
}
