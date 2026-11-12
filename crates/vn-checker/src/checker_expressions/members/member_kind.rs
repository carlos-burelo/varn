fn nested(
    k: vn_sem::semantic_info::NestedTypeKind,
) -> vn_sem::semantic_info::ResolvedMemberKind {
    vn_sem::semantic_info::ResolvedMemberKind::NestedType(k)
}

pub(super) fn map_class_member_kind(
    k: vn_sem::types::ClassMemberKind,
) -> vn_sem::semantic_info::ResolvedMemberKind {
    match k {
        vn_sem::types::ClassMemberKind::Method | vn_sem::types::ClassMemberKind::Function => {
            vn_sem::semantic_info::ResolvedMemberKind::Method
        }
        vn_sem::types::ClassMemberKind::Property | vn_sem::types::ClassMemberKind::Variable => {
            vn_sem::semantic_info::ResolvedMemberKind::Property
        }
        vn_sem::types::ClassMemberKind::Constructor => {
            vn_sem::semantic_info::ResolvedMemberKind::Constructor
        }
        vn_sem::types::ClassMemberKind::Getter => {
            vn_sem::semantic_info::ResolvedMemberKind::Getter
        }
        vn_sem::types::ClassMemberKind::Setter => {
            vn_sem::semantic_info::ResolvedMemberKind::Setter
        }
        vn_sem::types::ClassMemberKind::Class => {
            nested(vn_sem::semantic_info::NestedTypeKind::Class)
        }
        vn_sem::types::ClassMemberKind::Interface => {
            nested(vn_sem::semantic_info::NestedTypeKind::Interface)
        }
        vn_sem::types::ClassMemberKind::Namespace => {
            nested(vn_sem::semantic_info::NestedTypeKind::Namespace)
        }
        vn_sem::types::ClassMemberKind::Enum => {
            nested(vn_sem::semantic_info::NestedTypeKind::Enum)
        }
    }
}
