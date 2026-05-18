mod declarations;
mod extensions;
mod namespace_struct;
mod object_members;
mod patterns_sum;

use varn_core::ast::TypeNode;

pub(super) fn type_node_to_name(node: &TypeNode) -> String {
    use varn_core::{IntrinsicType, TypeKind, TypeTag};
    match &node.kind {
        TypeKind::Intrinsic(TypeTag::Int) => IntrinsicType::Int.as_str().to_owned(),
        TypeKind::Intrinsic(TypeTag::Float) => IntrinsicType::Float.as_str().to_owned(),
        TypeKind::Intrinsic(TypeTag::Str) => IntrinsicType::Str.as_str().to_owned(),
        TypeKind::Intrinsic(TypeTag::Bool) => IntrinsicType::Bool.as_str().to_owned(),
        TypeKind::Intrinsic(TypeTag::Char) => IntrinsicType::Char.as_str().to_owned(),
        TypeKind::Named(n, _) => n.clone(),
        TypeKind::Generic(n, _, _) => n.clone(),
        TypeKind::Intrinsic(TypeTag::Array) => IntrinsicType::Array.as_str().to_owned(),
        _ => IntrinsicType::Dynamic.as_str().to_owned(),
    }
}
