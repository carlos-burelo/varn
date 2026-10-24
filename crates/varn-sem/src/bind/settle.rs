use crate::symbol::SymbolId;
use crate::types::{ClassMemberInfo, ClassMemberKind, Type};
use rustc_hash::FxHashMap;

pub fn settle_members(members: &mut [ClassMemberInfo], symbol_types: &FxHashMap<SymbolId, Type>) {
    for member in members {
        match member.kind {
            ClassMemberKind::Variable => {
                if member.ty.is_dynamic()
                    && let Some(ty) = member.symbol_id.and_then(|id| symbol_types.get(&id))
                {
                    member.ty = *ty;
                }
            }
            ClassMemberKind::Namespace => settle_members(&mut member.members, symbol_types),
            ClassMemberKind::Method
            | ClassMemberKind::Function
            | ClassMemberKind::Property
            | ClassMemberKind::Constructor
            | ClassMemberKind::Getter
            | ClassMemberKind::Setter
            | ClassMemberKind::Class
            | ClassMemberKind::Interface
            | ClassMemberKind::Enum
            | ClassMemberKind::Struct => {}
        }
    }
}
