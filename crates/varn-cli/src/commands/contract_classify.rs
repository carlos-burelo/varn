use varn_core::ast::TypeNode;
use varn_core::kinds::TypeKind;
use varn_core::{AtomInterner, LangPrimitive};

fn scalar_code(p: LangPrimitive) -> &'static str {
    match p {
        LangPrimitive::Int => "int",
        LangPrimitive::Float => "float",
        LangPrimitive::Bool => "bool",
        LangPrimitive::Char => "char",
        LangPrimitive::Str => "str",
        LangPrimitive::Void => "void",
        LangPrimitive::Null
        | LangPrimitive::BigInt
        | LangPrimitive::Decimal
        | LangPrimitive::Never
        | LangPrimitive::Dynamic => "dynamic",
    }
}

pub(crate) fn opt_code(inner: String) -> String {
    format!("opt({inner})")
}

pub(crate) fn classify_code(t: &TypeNode, interner: &AtomInterner) -> String {
    match &t.kind {
        TypeKind::Named(n, _) => LangPrimitive::from_str(interner.resolve(*n))
            .map(scalar_code)
            .unwrap_or("dynamic")
            .to_string(),
        TypeKind::Primitive(LangPrimitive::Void) => "void".to_string(),
        TypeKind::TypePredicate { .. } => "bool".to_string(),
        TypeKind::Array(_) => "array".to_string(),
        TypeKind::Union(members) if members.len() == 2 => {
            if matches!(members[1].kind, TypeKind::Primitive(LangPrimitive::Null)) {
                opt_code(classify_code(&members[0], interner))
            } else if matches!(members[0].kind, TypeKind::Primitive(LangPrimitive::Null)) {
                opt_code(classify_code(&members[1], interner))
            } else {
                "dynamic".to_string()
            }
        }
        TypeKind::Primitive(_)
        | TypeKind::Builtin(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::Generic(..)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Object(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. } => "dynamic".to_string(),
    }
}
