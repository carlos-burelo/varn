mod declarations;
mod extensions;
mod namespace_struct;
mod object_members;
mod patterns_sum;

use varn_core::ast::TypeNode;

pub(super) fn type_node_to_name(node: &TypeNode, interner: &varn_core::AtomInterner) -> String {
    use varn_core::{IntrinsicType, TypeKind};
    match &node.kind {
        TypeKind::Primitive(varn_core::LangPrimitive::Int) => IntrinsicType::Int.as_str().to_owned(),
        TypeKind::Primitive(varn_core::LangPrimitive::Float) => IntrinsicType::Float.as_str().to_owned(),
        TypeKind::Primitive(varn_core::LangPrimitive::Str) => IntrinsicType::Str.as_str().to_owned(),
        TypeKind::Primitive(varn_core::LangPrimitive::Bool) => IntrinsicType::Bool.as_str().to_owned(),
        TypeKind::Primitive(varn_core::LangPrimitive::Char) => IntrinsicType::Char.as_str().to_owned(),
        TypeKind::Named(n, _) => interner.resolve(*n).to_owned(),
        TypeKind::Generic(n, _, _) => interner.resolve(*n).to_owned(),
        TypeKind::Builtin(varn_core::BuiltinType::Array) => IntrinsicType::Array.as_str().to_owned(),
        _ => IntrinsicType::Dynamic.as_str().to_owned(),
    }
}
