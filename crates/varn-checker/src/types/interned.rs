//! Hash-consed type table for the checker (Fase 1, Componente 3).
//!
//! `varn_core::TypeKind<T, N, C, F, O, E>` is generic over how recursion,
//! names and collections are represented. This module fixes those
//! parameters to interned handles instead of owned/boxed data:
//!
//! - `T` (recursion, e.g. `Array(T)`, `KeyOf(T)`)      -> `CheckerTyId`
//! - `N` (name, e.g. `Named(N, Option<N>)`)              -> `varn_core::Atom`
//! - `C` (collection, e.g. `Union(C)`, `Tuple(C)`)       -> `TyListId`
//! - `F` (function shape)                                -> `FunctionTypeId`
//! - `O` (object members)                                -> `ObjectMembersId`
//! - `E` (today `()` in `SemanticTypeKind`)              -> `()`
//!
//! `FunctionTypeId`/`ObjectMembersId` are reserved here (Task 19/20) and
//! populated once `types/mod.rs` migrates `FunctionType`/`ObjectTypeMember`
//! to reference `CheckerTyId` (Task 21) — this table does not intern their
//! contents yet, only allocates the id space so `InternedTypeKind` can
//! mention them today without a second breaking change later.

use crate::types::{FunctionType, ObjectTypeMember};
use rustc_hash::FxHashMap;
use varn_core::{Atom, TypeKind, TypeTag};

// `serde` derives here are a KNOWN CAVEAT, not an endorsement: like `Atom`, a
// bare interned index is meaningless without the matching `CheckerTyTable` —
// serializing one (e.g. through `module_resolver::cache`'s on-disk `BindResult`
// cache, which stores `Type`-bearing `TypeMembers`/`ClassMemberInfo` today)
// round-trips a number, not a type, unless the table that produced it is
// reconstructed identically before deserializing. `Atom` solved this by NOT
// deriving `serde` and pushing every carrier to `#[serde(skip)]`; doing the
// same for `CheckerTyId` is out of this task's scope (Task 21 migrates
// `types/{mod,type_impl,object_member_impl,class_member_impl}.rs`, not the
// module cache) — deriving `serde` here keeps today's cache code compiling
// and defers the correctness fix to whichever task next touches
// `module_resolver::cache`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CheckerTyId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TyListId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FunctionTypeId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ObjectMembersId(u32);

/// Interned form of `checker::types::SemanticTypeKind`. Same 19 variants as
/// `varn_core::TypeKind` (confirmed by reading `kinds.rs` in full — the plan
/// draft omitted `TypePredicate`), parameters substituted per the module doc
/// above. `Copy` because every substituted parameter is `Copy`.
pub type InternedTypeKind = TypeKind<CheckerTyId, Atom, TyListId, FunctionTypeId, ObjectMembersId, ()>;

/// Fixed ids for the ~21 zero-argument/intrinsic shapes every checker session
/// needs (the `Type::Int`/`Type::Str`/... constants `type_impl.rs` exposes).
/// `CheckerTyTable::new` interns them in EXACTLY this order — first call to
/// `intern` gets id 0, second gets id 1, etc — so these constants are valid
/// the instant a fresh table exists, with no `&mut CheckerTyTable` required
/// at every use site. Keeping the two lists (these consts, and the seeding
/// order in `new`) in sync is enforced by a `debug_assert_eq!` per entry in
/// `new`, so drift panics loudly in debug builds instead of silently handing
/// out the wrong constant.
impl CheckerTyId {
    pub const INT: CheckerTyId = CheckerTyId(0);
    pub const FLOAT: CheckerTyId = CheckerTyId(1);
    pub const DECIMAL: CheckerTyId = CheckerTyId(2);
    pub const BIGINT: CheckerTyId = CheckerTyId(3);
    pub const STR: CheckerTyId = CheckerTyId(4);
    pub const CHAR: CheckerTyId = CheckerTyId(5);
    pub const BOOL: CheckerTyId = CheckerTyId(6);
    pub const SYMBOL: CheckerTyId = CheckerTyId(7);
    pub const VOID: CheckerTyId = CheckerTyId(8);
    pub const NULL: CheckerTyId = CheckerTyId(9);
    pub const NEVER: CheckerTyId = CheckerTyId(10);
    pub const DYNAMIC: CheckerTyId = CheckerTyId(11);
    pub const I8: CheckerTyId = CheckerTyId(12);
    pub const I16: CheckerTyId = CheckerTyId(13);
    pub const I32: CheckerTyId = CheckerTyId(14);
    pub const U8: CheckerTyId = CheckerTyId(15);
    pub const U16: CheckerTyId = CheckerTyId(16);
    pub const U32: CheckerTyId = CheckerTyId(17);
    pub const U64: CheckerTyId = CheckerTyId(18);
    pub const F32: CheckerTyId = CheckerTyId(19);
    pub const THIS: CheckerTyId = CheckerTyId(20);
}

#[derive(Debug, Clone)]
pub struct CheckerTyTable {
    entries: Vec<InternedTypeKind>,
    dedup: FxHashMap<InternedTypeKind, u32>,
    lists: Vec<Vec<CheckerTyId>>,
    list_dedup: FxHashMap<Vec<CheckerTyId>, u32>,
    functions: Vec<FunctionType>,
    function_dedup: FxHashMap<FunctionType, u32>,
    object_members: Vec<Vec<ObjectTypeMember>>,
    object_dedup: FxHashMap<Vec<ObjectTypeMember>, u32>,
}

impl Default for CheckerTyTable {
    fn default() -> Self {
        Self::new()
    }
}

impl CheckerTyTable {
    pub fn new() -> Self {
        let mut t = Self {
            entries: Vec::new(),
            dedup: FxHashMap::default(),
            lists: Vec::new(),
            list_dedup: FxHashMap::default(),
            functions: Vec::new(),
            function_dedup: FxHashMap::default(),
            object_members: Vec::new(),
            object_dedup: FxHashMap::default(),
        };
        let seed = [
            (CheckerTyId::INT, TypeKind::Intrinsic(TypeTag::Int)),
            (CheckerTyId::FLOAT, TypeKind::Intrinsic(TypeTag::Float)),
            (CheckerTyId::DECIMAL, TypeKind::Intrinsic(TypeTag::Decimal)),
            (CheckerTyId::BIGINT, TypeKind::Intrinsic(TypeTag::BigInt)),
            (CheckerTyId::STR, TypeKind::Intrinsic(TypeTag::Str)),
            (CheckerTyId::CHAR, TypeKind::Intrinsic(TypeTag::Char)),
            (CheckerTyId::BOOL, TypeKind::Intrinsic(TypeTag::Bool)),
            (CheckerTyId::SYMBOL, TypeKind::Intrinsic(TypeTag::Symbol)),
            (CheckerTyId::VOID, TypeKind::Intrinsic(TypeTag::Void)),
            (CheckerTyId::NULL, TypeKind::Intrinsic(TypeTag::Null)),
            (CheckerTyId::NEVER, TypeKind::Intrinsic(TypeTag::Never)),
            (CheckerTyId::DYNAMIC, TypeKind::Intrinsic(TypeTag::Dynamic)),
            (CheckerTyId::I8, TypeKind::Intrinsic(TypeTag::I8)),
            (CheckerTyId::I16, TypeKind::Intrinsic(TypeTag::I16)),
            (CheckerTyId::I32, TypeKind::Intrinsic(TypeTag::I32)),
            (CheckerTyId::U8, TypeKind::Intrinsic(TypeTag::U8)),
            (CheckerTyId::U16, TypeKind::Intrinsic(TypeTag::U16)),
            (CheckerTyId::U32, TypeKind::Intrinsic(TypeTag::U32)),
            (CheckerTyId::U64, TypeKind::Intrinsic(TypeTag::U64)),
            (CheckerTyId::F32, TypeKind::Intrinsic(TypeTag::F32)),
            (CheckerTyId::THIS, TypeKind::This),
        ];
        for (expected, kind) in seed {
            let got = t.intern(kind);
            debug_assert_eq!(
                got, expected,
                "CheckerTyTable::new: intrinsic seeding order drifted from the CheckerTyId consts"
            );
        }
        t
    }

    pub fn intern(&mut self, kind: InternedTypeKind) -> CheckerTyId {
        if let Some(&i) = self.dedup.get(&kind) {
            return CheckerTyId(i);
        }
        let i = self.entries.len() as u32;
        self.entries.push(kind);
        self.dedup.insert(kind, i);
        CheckerTyId(i)
    }

    pub fn get(&self, id: CheckerTyId) -> &InternedTypeKind {
        &self.entries[id.0 as usize]
    }

    pub fn intern_list(&mut self, tys: &[CheckerTyId]) -> TyListId {
        if let Some(&i) = self.list_dedup.get(tys) {
            return TyListId(i);
        }
        let i = self.lists.len() as u32;
        self.lists.push(tys.to_vec());
        self.list_dedup.insert(tys.to_vec(), i);
        TyListId(i)
    }

    pub fn get_list(&self, id: TyListId) -> &[CheckerTyId] {
        &self.lists[id.0 as usize]
    }

    pub fn intern_function(&mut self, f: FunctionType) -> FunctionTypeId {
        if let Some(&i) = self.function_dedup.get(&f) {
            return FunctionTypeId(i);
        }
        let i = self.functions.len() as u32;
        self.functions.push(f.clone());
        self.function_dedup.insert(f, i);
        FunctionTypeId(i)
    }

    pub fn get_function(&self, id: FunctionTypeId) -> &FunctionType {
        &self.functions[id.0 as usize]
    }

    pub fn intern_object_members(&mut self, members: Vec<ObjectTypeMember>) -> ObjectMembersId {
        if let Some(&i) = self.object_dedup.get(&members) {
            return ObjectMembersId(i);
        }
        let i = self.object_members.len() as u32;
        self.object_members.push(members.clone());
        self.object_dedup.insert(members, i);
        ObjectMembersId(i)
    }

    pub fn get_object_members(&self, id: ObjectMembersId) -> &[ObjectTypeMember] {
        &self.object_members[id.0 as usize]
    }

    /// Number of distinct interned shapes — used the same way
    /// `AtomInterner::len` is used by `DiskResolver::set_interner`: to decide
    /// whether an incoming table is a superset-by-prefix of the live one
    /// (grew from it) rather than a stale, smaller snapshot.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use varn_core::TypeTag;

    #[test]
    fn interning_the_same_intrinsic_twice_dedups() {
        let mut t = CheckerTyTable::default();
        let a = t.intern(TypeKind::Intrinsic(TypeTag::Int));
        let b = t.intern(TypeKind::Intrinsic(TypeTag::Int));
        assert_eq!(a, b);
    }

    #[test]
    fn different_intrinsics_get_different_ids() {
        let mut t = CheckerTyTable::default();
        let a = t.intern(TypeKind::Intrinsic(TypeTag::Int));
        let b = t.intern(TypeKind::Intrinsic(TypeTag::Str));
        assert_ne!(a, b);
    }

    #[test]
    fn get_roundtrips_the_interned_value() {
        let mut t = CheckerTyTable::default();
        let a = t.intern(TypeKind::Intrinsic(TypeTag::Bool));
        assert_eq!(*t.get(a), TypeKind::Intrinsic(TypeTag::Bool));
    }

    #[test]
    fn identical_unions_by_member_ids_dedup_via_ty_list() {
        let mut t = CheckerTyTable::default();
        let int = t.intern(TypeKind::Intrinsic(TypeTag::Int));
        let str_ = t.intern(TypeKind::Intrinsic(TypeTag::Str));
        let list1 = t.intern_list(&[int, str_]);
        let list2 = t.intern_list(&[int, str_]);
        assert_eq!(list1, list2);
        let union1 = t.intern(TypeKind::Union(list1));
        let union2 = t.intern(TypeKind::Union(list2));
        assert_eq!(union1, union2);
    }
}
