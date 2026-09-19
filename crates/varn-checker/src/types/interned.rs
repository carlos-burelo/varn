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

use rustc_hash::FxHashMap;
use varn_core::{Atom, TypeKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CheckerTyId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TyListId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FunctionTypeId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjectMembersId(u32);

/// Interned form of `checker::types::SemanticTypeKind`. Same 19 variants as
/// `varn_core::TypeKind` (confirmed by reading `kinds.rs` in full — the plan
/// draft omitted `TypePredicate`), parameters substituted per the module doc
/// above. `Copy` because every substituted parameter is `Copy`.
pub type InternedTypeKind = TypeKind<CheckerTyId, Atom, TyListId, FunctionTypeId, ObjectMembersId, ()>;

#[derive(Debug, Default)]
pub struct CheckerTyTable {
    entries: Vec<InternedTypeKind>,
    dedup: FxHashMap<InternedTypeKind, u32>,
    lists: Vec<Vec<CheckerTyId>>,
    list_dedup: FxHashMap<Vec<CheckerTyId>, u32>,
}

impl CheckerTyTable {
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
