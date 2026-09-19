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

    /// The highest id `CheckerTyTable::new()`'s fixed seeding ever assigns —
    /// everything `<= THIS.0` names the same intrinsic shape in *any* table,
    /// since seeding order is fixed and `debug_assert!`-enforced. Anything
    /// past it is table-relative and portable only within the table that
    /// interned it.
    const MAX_PORTABLE: u32 = Self::THIS.0;

    /// `true` for exactly the ~21 fixed intrinsic ids every `CheckerTyTable`
    /// seeds identically — the only `CheckerTyId`s that mean the same thing
    /// in a table other than the one that produced them.
    pub fn is_portable(self) -> bool {
        self.0 <= Self::MAX_PORTABLE
    }

    /// `self` if it's one of the ~21 ids valid in any table, else
    /// [`Self::DYNAMIC`] — the honest degradation for a `CheckerTyId` that
    /// crossed into a table it wasn't interned in (the on-disk module cache,
    /// `module_resolver::cache.rs`, is the one place this happens today: see
    /// its own doc for why a non-intrinsic cached type can't be reconstructed
    /// without a portable encoding this crate doesn't have yet). Matches the
    /// rest of the checker's own philosophy for a type it genuinely doesn't
    /// know (`BackendTy::Dynamic(DynReason::Unannotated)` in `emit/ty.rs`) —
    /// wrong-but-plausible would be worse than honestly unknown.
    pub fn sanitize_foreign(self) -> CheckerTyId {
        if self.is_portable() {
            self
        } else {
            Self::DYNAMIC
        }
    }
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

    /// Copy `id`'s shape from `other` into `self`, recursively, returning the
    /// id it now has *in `self`*. `Atom`/`TypeTag` payloads pass through
    /// unchanged (they're already `self`-relative — see the reintern helpers
    /// that call this) but a `CheckerTyId` embedded inside a shape (an array's
    /// element, a union's members, a function's params/return, ...) is only
    /// meaningful relative to the table it was interned in — copying the raw
    /// id across tables the way `Symbol::name`/`origin_module` copy an `Atom`
    /// would silently point at whatever shape happens to sit at that index in
    /// `self`. Needed anywhere a `Type` crosses from one table's lineage into
    /// another's, e.g. reconstructing an imported `Symbol` whose `ty` was
    /// interned by the module that exports it (`binder/imports.rs`).
    ///
    /// `cache` carries entries across sibling calls within one reintern (not
    /// just recursive ones) so a shape referenced twice — a diamond, or the
    /// same member type repeated in a union — is translated once.
    pub fn reintern(
        &mut self,
        other: &CheckerTyTable,
        id: CheckerTyId,
        cache: &mut FxHashMap<CheckerTyId, CheckerTyId>,
    ) -> CheckerTyId {
        if let Some(&done) = cache.get(&id) {
            return done;
        }
        let kind = *other.get(id);
        let translated = match kind {
            TypeKind::Intrinsic(tag) => TypeKind::Intrinsic(tag),
            TypeKind::This => TypeKind::This,
            TypeKind::Array(inner) => TypeKind::Array(self.reintern(other, inner, cache)),
            TypeKind::Union(list) => TypeKind::Union(self.reintern_list(other, list, cache)),
            TypeKind::Intersection(list) => {
                TypeKind::Intersection(self.reintern_list(other, list, cache))
            }
            TypeKind::Tuple(list) => TypeKind::Tuple(self.reintern_list(other, list, cache)),
            TypeKind::Named(n, o) => TypeKind::Named(n, o),
            TypeKind::Generic(n, list, o) => {
                TypeKind::Generic(n, self.reintern_list(other, list, cache), o)
            }
            TypeKind::TemplateLiteral(list) => {
                TypeKind::TemplateLiteral(self.reintern_list(other, list, cache))
            }
            TypeKind::Fn(fid) => TypeKind::Fn(self.reintern_function(other, fid, cache)),
            TypeKind::Object(oid) => TypeKind::Object(self.reintern_object_members(other, oid, cache)),
            TypeKind::Typeof(e) => TypeKind::Typeof(e),
            TypeKind::KeyOf(inner) => TypeKind::KeyOf(self.reintern(other, inner, cache)),
            TypeKind::IndexedAccess { object, index } => TypeKind::IndexedAccess {
                object: self.reintern(other, object, cache),
                index: self.reintern(other, index, cache),
            },
            TypeKind::Mapped {
                key_var,
                source,
                value,
                optional,
                readonly,
            } => TypeKind::Mapped {
                key_var,
                source: self.reintern(other, source, cache),
                value: self.reintern(other, value, cache),
                optional,
                readonly,
            },
            TypeKind::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => TypeKind::Conditional {
                check: self.reintern(other, check, cache),
                extends: self.reintern(other, extends, cache),
                true_type: self.reintern(other, true_type, cache),
                false_type: self.reintern(other, false_type, cache),
            },
            TypeKind::Infer(n) => TypeKind::Infer(n),
            TypeKind::EnumVariant {
                enum_name,
                variant_name,
                type_args,
                payload_ty,
            } => TypeKind::EnumVariant {
                enum_name,
                variant_name,
                type_args: self.reintern_list(other, type_args, cache),
                payload_ty: self.reintern(other, payload_ty, cache),
            },
            TypeKind::TypePredicate {
                parameter_name,
                target_type,
            } => TypeKind::TypePredicate {
                parameter_name,
                target_type: self.reintern(other, target_type, cache),
            },
        };
        let new_id = self.intern(translated);
        cache.insert(id, new_id);
        new_id
    }

    fn reintern_list(
        &mut self,
        other: &CheckerTyTable,
        id: TyListId,
        cache: &mut FxHashMap<CheckerTyId, CheckerTyId>,
    ) -> TyListId {
        let translated: Vec<CheckerTyId> = other
            .get_list(id)
            .to_vec()
            .into_iter()
            .map(|t| self.reintern(other, t, cache))
            .collect();
        self.intern_list(&translated)
    }

    fn reintern_function(
        &mut self,
        other: &CheckerTyTable,
        id: FunctionTypeId,
        cache: &mut FxHashMap<CheckerTyId, CheckerTyId>,
    ) -> FunctionTypeId {
        let f = other.get_function(id).clone();
        let params = f
            .params
            .into_iter()
            .map(|p| crate::types::FunctionParam {
                name: p.name,
                ty: self.reintern(other, p.ty, cache),
                optional: p.optional,
                is_rest: p.is_rest,
            })
            .collect();
        let return_type = self.reintern(other, f.return_type, cache);
        self.intern_function(crate::types::FunctionType {
            params,
            return_type,
            is_arrow: f.is_arrow,
            type_params: f.type_params,
        })
    }

    fn reintern_object_members(
        &mut self,
        other: &CheckerTyTable,
        id: ObjectMembersId,
        cache: &mut FxHashMap<CheckerTyId, CheckerTyId>,
    ) -> ObjectMembersId {
        use crate::types::ObjectTypeMember as M;
        let members: Vec<M> = other
            .get_object_members(id)
            .to_vec()
            .into_iter()
            .map(|m| match m {
                M::Property {
                    name,
                    ty,
                    optional,
                    readonly,
                } => M::Property {
                    name,
                    ty: self.reintern(other, ty, cache),
                    optional,
                    readonly,
                },
                M::Method {
                    name,
                    params,
                    return_type,
                    optional,
                    is_arrow,
                } => M::Method {
                    name,
                    params: params
                        .into_iter()
                        .map(|p| crate::types::FunctionParam {
                            name: p.name,
                            ty: self.reintern(other, p.ty, cache),
                            optional: p.optional,
                            is_rest: p.is_rest,
                        })
                        .collect(),
                    return_type: self.reintern(other, return_type, cache),
                    optional,
                    is_arrow,
                },
                M::Index {
                    param_name,
                    key_ty,
                    value_ty,
                } => M::Index {
                    param_name,
                    key_ty: self.reintern(other, key_ty, cache),
                    value_ty: self.reintern(other, value_ty, cache),
                },
                M::Callable {
                    params,
                    return_type,
                    is_arrow,
                } => M::Callable {
                    params: params
                        .into_iter()
                        .map(|p| crate::types::FunctionParam {
                            name: p.name,
                            ty: self.reintern(other, p.ty, cache),
                            optional: p.optional,
                            is_rest: p.is_rest,
                        })
                        .collect(),
                    return_type: self.reintern(other, return_type, cache),
                    is_arrow,
                },
            })
            .collect();
        self.intern_object_members(members)
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
