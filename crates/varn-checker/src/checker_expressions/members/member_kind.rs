fn nested(
    k: varn_sem::semantic_info::NestedTypeKind,
) -> varn_sem::semantic_info::ResolvedMemberKind {
    varn_sem::semantic_info::ResolvedMemberKind::NestedType(k)
}

pub(super) fn map_class_member_kind(
    k: varn_sem::types::ClassMemberKind,
) -> varn_sem::semantic_info::ResolvedMemberKind {
    match k {
        varn_sem::types::ClassMemberKind::Method | varn_sem::types::ClassMemberKind::Function => {
            varn_sem::semantic_info::ResolvedMemberKind::Method
        }
        varn_sem::types::ClassMemberKind::Property | varn_sem::types::ClassMemberKind::Variable => {
            varn_sem::semantic_info::ResolvedMemberKind::Property
        }
        varn_sem::types::ClassMemberKind::Constructor => {
            varn_sem::semantic_info::ResolvedMemberKind::Constructor
        }
        varn_sem::types::ClassMemberKind::Getter => {
            varn_sem::semantic_info::ResolvedMemberKind::Getter
        }
        varn_sem::types::ClassMemberKind::Setter => {
            varn_sem::semantic_info::ResolvedMemberKind::Setter
        }
        varn_sem::types::ClassMemberKind::Class => {
            nested(varn_sem::semantic_info::NestedTypeKind::Class)
        }
        varn_sem::types::ClassMemberKind::Interface => {
            nested(varn_sem::semantic_info::NestedTypeKind::Interface)
        }
        varn_sem::types::ClassMemberKind::Namespace => {
            nested(varn_sem::semantic_info::NestedTypeKind::Namespace)
        }
        varn_sem::types::ClassMemberKind::Enum => {
            nested(varn_sem::semantic_info::NestedTypeKind::Enum)
        }
        varn_sem::types::ClassMemberKind::Struct => {
            nested(varn_sem::semantic_info::NestedTypeKind::Struct)
        }
    }
}
