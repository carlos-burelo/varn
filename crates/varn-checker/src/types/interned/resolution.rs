use super::ids::{CheckerTyId, FunctionTypeId, InternedTypeKind, ObjectMembersId, TyListId};
use super::table::CheckerTyTable;
use crate::types::{FunctionType, ObjectTypeMember};

impl CheckerTyTable {
    
    
    pub fn get(&self, id: CheckerTyId) -> InternedTypeKind {
        *self
            .base
            .entries
            .get(&id)
            .or_else(|| self.delta_entries.get(&id))
            .unwrap_or_else(|| panic!("CheckerTyId {id:?} is not present in this table"))
    }

    
    pub fn contains(&self, id: CheckerTyId) -> bool {
        self.base.entries.contains_key(&id) || self.delta_entries.contains_key(&id)
    }

    pub fn get_list(&self, id: TyListId) -> &[CheckerTyId] {
        self.base
            .lists
            .get(&id)
            .or_else(|| self.delta_lists.get(&id))
            .map(Vec::as_slice)
            .unwrap_or_else(|| panic!("TyListId {id:?} is not present in this table"))
    }

    pub fn get_function(&self, id: FunctionTypeId) -> &FunctionType {
        self.base
            .functions
            .get(&id)
            .or_else(|| self.delta_functions.get(&id))
            .unwrap_or_else(|| panic!("FunctionTypeId {id:?} is not present in this table"))
    }

    pub fn get_object_members(&self, id: ObjectMembersId) -> &[ObjectTypeMember] {
        self.base
            .object_members
            .get(&id)
            .or_else(|| self.delta_object_members.get(&id))
            .map(Vec::as_slice)
            .unwrap_or_else(|| panic!("ObjectMembersId {id:?} is not present in this table"))
    }

    
    
    
    
    pub fn contains_object_members(&self, id: ObjectMembersId) -> bool {
        self.base.object_members.contains_key(&id) || self.delta_object_members.contains_key(&id)
    }
}
