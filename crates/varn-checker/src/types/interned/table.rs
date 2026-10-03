use super::ids::{CheckerTyId, FunctionTypeId, ObjectMembersId, TyListId};
use crate::types::{FunctionType, ObjectTypeMember};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::{LangPrimitive, TypeKind};

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
pub(super) struct CheckerTyBase {
    pub(super) entries: FxHashMap<CheckerTyId, super::ids::InternedTypeKind>,
    pub(super) lists: FxHashMap<TyListId, Vec<CheckerTyId>>,
    pub(super) functions: FxHashMap<FunctionTypeId, FunctionType>,
    pub(super) object_members: FxHashMap<ObjectMembersId, Vec<ObjectTypeMember>>,
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
    pub(super) base: Arc<CheckerTyBase>,
    pub(super) delta_entries: FxHashMap<CheckerTyId, super::ids::InternedTypeKind>,
    pub(super) delta_lists: FxHashMap<TyListId, Vec<CheckerTyId>>,
    pub(super) delta_functions: FxHashMap<FunctionTypeId, FunctionType>,
    pub(super) delta_object_members: FxHashMap<ObjectMembersId, Vec<ObjectTypeMember>>,
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
                TypeKind::Primitive(LangPrimitive::Int),
            ),
            (
                CheckerTyId::FLOAT,
                TypeKind::Primitive(LangPrimitive::Float),
            ),
            (
                CheckerTyId::DECIMAL,
                TypeKind::Primitive(LangPrimitive::Decimal),
            ),
            (
                CheckerTyId::BIGINT,
                TypeKind::Primitive(LangPrimitive::BigInt),
            ),
            (
                CheckerTyId::STR,
                TypeKind::Primitive(LangPrimitive::Str),
            ),
            (
                CheckerTyId::CHAR,
                TypeKind::Primitive(LangPrimitive::Char),
            ),
            (
                CheckerTyId::BOOL,
                TypeKind::Primitive(LangPrimitive::Bool),
            ),
            (
                CheckerTyId::VOID,
                TypeKind::Primitive(LangPrimitive::Void),
            ),
            (
                CheckerTyId::NULL,
                TypeKind::Primitive(LangPrimitive::Null),
            ),
            (
                CheckerTyId::NEVER,
                TypeKind::Primitive(LangPrimitive::Never),
            ),
            (
                CheckerTyId::DYNAMIC,
                TypeKind::Primitive(LangPrimitive::Dynamic),
            ),
            (CheckerTyId::THIS, TypeKind::This),
        ] {
            base.entries.insert(id, kind);
        }
        Self {
            base: Arc::new(base),
            delta_entries: FxHashMap::default(),
            delta_lists: FxHashMap::default(),
            delta_functions: FxHashMap::default(),
            delta_object_members: FxHashMap::default(),
        }
    }

    /// Fold every delta map into a fresh frozen `base`. O(n) in the total
    /// table size, but only runs once every `FREEZE_THRESHOLD` new entries
    /// across all four maps combined, not once per caller.
    pub(super) fn freeze(&mut self) {
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
        self.base = Arc::new(CheckerTyBase {
            entries,
            lists,
            functions,
            object_members,
        });
    }

    pub(super) fn maybe_freeze(&mut self) {
        let delta_size = self.delta_entries.len()
            + self.delta_lists.len()
            + self.delta_functions.len()
            + self.delta_object_members.len();
        if delta_size >= FREEZE_THRESHOLD {
            self.freeze();
        }
    }

    /// Number of distinct interned shapes. Used as a cheap "did the live table
    /// grow past this snapshot" heuristic by `Binder::sync_ty_table`.
    pub fn len(&self) -> usize {
        self.base.entries.len() + self.delta_entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_the_same_intrinsic_twice_dedups() {
        let mut t = CheckerTyTable::default();
        let a = t.intern(TypeKind::Primitive(LangPrimitive::Int));
        let b = t.intern(TypeKind::Primitive(LangPrimitive::Int));
        assert_eq!(a, b);
    }

    #[test]
    fn different_intrinsics_get_different_ids() {
        let mut t = CheckerTyTable::default();
        let a = t.intern(TypeKind::Primitive(LangPrimitive::Int));
        let b = t.intern(TypeKind::Primitive(LangPrimitive::Str));
        assert_ne!(a, b);
    }

    #[test]
    fn get_roundtrips_the_interned_value() {
        let mut t = CheckerTyTable::default();
        let a = t.intern(TypeKind::Primitive(LangPrimitive::Bool));
        assert_eq!(
            t.get(a),
            TypeKind::Primitive(LangPrimitive::Bool)
        );
    }

    #[test]
    fn identical_unions_by_member_ids_dedup_via_ty_list() {
        let mut t = CheckerTyTable::default();
        let int = t.intern(TypeKind::Primitive(LangPrimitive::Int));
        let str_ = t.intern(TypeKind::Primitive(LangPrimitive::Str));
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
        let l_int = left.intern(TypeKind::Primitive(LangPrimitive::Int));
        let l_arr = left.intern(TypeKind::Array(l_int));
        let r_int = right.intern(TypeKind::Primitive(LangPrimitive::Int));
        let r_arr = right.intern(TypeKind::Array(r_int));

        assert_eq!(l_int, CheckerTyId::INT);
        assert_eq!(l_arr, r_arr, "same shape -> same id regardless of order");
    }

    #[test]
    fn interning_past_freeze_threshold_still_resolves_correctly() {
        let mut t = CheckerTyTable::default();
        let mut ids = Vec::new();
        for i in 0..(FREEZE_THRESHOLD * 2 + 3) {
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
        assert!(
            local.delta_entries.is_empty()
                && local.delta_lists.is_empty()
                && local.delta_functions.is_empty()
                && local.delta_object_members.is_empty(),
            "absorb left an oversized delta instead of freezing it"
        );
    }

    /// `absorb`'s `Arc::ptr_eq` shortcut must never skip content that is
    /// genuinely new: when `other` shares `self`'s frozen base but has grown
    /// its own delta since the clone, that delta is exactly what `self` is
    /// missing and must still be copied over.
    #[test]
    fn absorb_with_shared_base_still_picks_up_the_other_deltas_new_entries() {
        let mut local = CheckerTyTable::default();
        let local_id = local.intern_object_members(vec![ObjectTypeMember::Property {
            name: std::sync::Arc::from("local-only"),
            ty: CheckerTyId::INT,
            optional: false,
            readonly: false,
        }]);
        let mut clone = local.clone();
        assert!(std::sync::Arc::ptr_eq(&local.base, &clone.base));
        let clone_only_id = clone.intern_object_members(vec![ObjectTypeMember::Property {
            name: std::sync::Arc::from("clone-only"),
            ty: CheckerTyId::INT,
            optional: false,
            readonly: false,
        }]);

        local.absorb(&clone);

        assert!(local.contains_object_members(local_id));
        assert!(
            local.contains_object_members(clone_only_id),
            "absorb must still pick up entries from other's delta even when bases are shared"
        );
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
