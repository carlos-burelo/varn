fn nested(k: crate::semantic_info::NestedTypeKind) -> crate::semantic_info::ResolvedMemberKind {
    crate::semantic_info::ResolvedMemberKind::NestedType(k)
}

pub(super) fn map_class_member_kind(
    k: crate::binder::ClassMemberKind,
) -> crate::semantic_info::ResolvedMemberKind {
    match k {
        crate::binder::ClassMemberKind::Method | crate::binder::ClassMemberKind::Function => {
            crate::semantic_info::ResolvedMemberKind::Method
        }
        crate::binder::ClassMemberKind::Property | crate::binder::ClassMemberKind::Variable => {
            crate::semantic_info::ResolvedMemberKind::Property
        }
        crate::binder::ClassMemberKind::Constructor => {
            crate::semantic_info::ResolvedMemberKind::Constructor
        }
        crate::binder::ClassMemberKind::Getter => crate::semantic_info::ResolvedMemberKind::Getter,
        crate::binder::ClassMemberKind::Setter => crate::semantic_info::ResolvedMemberKind::Setter,
        crate::binder::ClassMemberKind::Class => {
            nested(crate::semantic_info::NestedTypeKind::Class)
        }
        crate::binder::ClassMemberKind::Interface => {
            nested(crate::semantic_info::NestedTypeKind::Interface)
        }
        crate::binder::ClassMemberKind::Namespace => {
            nested(crate::semantic_info::NestedTypeKind::Namespace)
        }
        crate::binder::ClassMemberKind::Enum => nested(crate::semantic_info::NestedTypeKind::Enum),
        crate::binder::ClassMemberKind::Struct => {
            nested(crate::semantic_info::NestedTypeKind::Struct)
        }
    }
}
