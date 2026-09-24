mod declarations;
mod extensions;
mod namespace_struct;
mod object_members;
mod patterns_sum;

use varn_core::ast::TypeNode;

pub(super) fn type_node_to_name(node: &TypeNode, interner: &varn_core::AtomInterner) -> String {
    use varn_core::{TypeKind};
    match &node.kind {
        TypeKind::Primitive(varn_core::LangPrimitive::Int) => varn_core::LangPrimitive::Int.name().to_owned(),
        TypeKind::Primitive(varn_core::LangPrimitive::Float) => varn_core::LangPrimitive::Float.name().to_owned(),
        TypeKind::Primitive(varn_core::LangPrimitive::Str) => varn_core::LangPrimitive::Str.name().to_owned(),
        TypeKind::Primitive(varn_core::LangPrimitive::Bool) => varn_core::LangPrimitive::Bool.name().to_owned(),
        TypeKind::Primitive(varn_core::LangPrimitive::Char) => varn_core::LangPrimitive::Char.name().to_owned(),
        TypeKind::Named(n, _) => interner.resolve(*n).to_owned(),
        TypeKind::Generic(n, _, _) => interner.resolve(*n).to_owned(),
        TypeKind::Builtin(varn_core::BuiltinType::Array) => varn_core::BuiltinType::Array.name().to_owned(),
        _ => varn_core::LangPrimitive::Dynamic.name().to_owned(),
    }
}
