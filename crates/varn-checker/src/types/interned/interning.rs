use super::hash::{content_function_id, content_id, content_list_id, content_object_id};
use super::ids::{
    seeded_id, CheckerTyId, FunctionTypeId, InternedTypeKind, ObjectMembersId, TyListId,
};
use super::table::CheckerTyTable;
use crate::types::{FunctionType, ObjectTypeMember};

impl CheckerTyTable {
    
    
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
}
