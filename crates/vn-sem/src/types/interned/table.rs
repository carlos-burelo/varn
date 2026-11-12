use super::ids::{CheckerTyId, FunctionTypeId, ObjectMembersId, TyListId};
use crate::types::{FunctionType, ObjectTypeMember};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use vn_core::{LangPrimitive, TypeKind};

const FREEZE_THRESHOLD: usize = 2048;

#[derive(Debug, Default)]
pub(super) struct CheckerTyBase {
    pub(super) entries: FxHashMap<CheckerTyId, super::ids::InternedTypeKind>,
    pub(super) lists: FxHashMap<TyListId, Vec<CheckerTyId>>,
    pub(super) functions: FxHashMap<FunctionTypeId, FunctionType>,
    pub(super) object_members: FxHashMap<ObjectMembersId, Vec<ObjectTypeMember>>,
}

#[derive(Debug, Clone)]
pub struct CheckerTyTable {
    pub(super) base: Arc<CheckerTyBase>,
    pub(super) delta_entries: FxHashMap<CheckerTyId, super::ids::InternedTypeKind>,
    pub(super) delta_lists: FxHashMap<TyListId, Vec<CheckerTyId>>,
    pub(super) delta_functions: FxHashMap<FunctionTypeId, FunctionType>,
    pub(super) delta_object_members: FxHashMap<ObjectMembersId, Vec<ObjectTypeMember>>,
    pub(super) names: vn_core::AtomInterner,
}

impl Default for CheckerTyTable {
    fn default() -> Self {
        Self::new()
    }
}

impl CheckerTyTable {
    pub fn new() -> Self {
        let mut base = CheckerTyBase::default();

        for (id, kind) in [
            (CheckerTyId::INT, TypeKind::Primitive(LangPrimitive::Int)),
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
            (CheckerTyId::STR, TypeKind::Primitive(LangPrimitive::Str)),
            (CheckerTyId::CHAR, TypeKind::Primitive(LangPrimitive::Char)),
            (CheckerTyId::BOOL, TypeKind::Primitive(LangPrimitive::Bool)),
            (CheckerTyId::VOID, TypeKind::Primitive(LangPrimitive::Void)),
            (CheckerTyId::NULL, TypeKind::Primitive(LangPrimitive::Null)),
            (
                CheckerTyId::NEVER,
                TypeKind::Primitive(LangPrimitive::Never),
            ),
            (
                CheckerTyId::DYNAMIC,
                TypeKind::Primitive(LangPrimitive::Dynamic),
            ),
            (
                CheckerTyId::ERROR,
                TypeKind::Primitive(LangPrimitive::Error),
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
            names: vn_core::AtomInterner::new(),
        }
    }

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

    pub fn len(&self) -> usize {
        self.base.entries.len() + self.delta_entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
