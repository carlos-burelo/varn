use crate::types::{Type, TypeContext};
use std::rc::Rc;
use varn_core::ast::TypeNode;
use varn_core::TypeKind;

use super::resolve_type_node;

pub(super) fn collect_string_literals(ty: &Type) -> Vec<Rc<str>> {
    match &ty.0 {
        TypeKind::LiteralStr(s) => vec![s.clone()],
        TypeKind::Union(members) => members.iter().flat_map(collect_string_literals).collect(),
        _ => vec![],
    }
}

fn collect_template_literal_variants(parts: &[Type]) -> Option<Vec<String>> {
    let mut variants: Vec<String> = vec![String::new()];
    for (idx, part) in parts.iter().enumerate() {
        if idx % 2 == 0 {
            if let TypeKind::LiteralStr(s) = &part.0 {
                for v in &mut variants {
                    v.push_str(s.as_ref());
                }
            } else {
                return None;
            }
            continue;
        }

        match &part.0 {
            TypeKind::LiteralStr(s) => {
                for v in &mut variants {
                    v.push_str(s.as_ref());
                }
            }
            TypeKind::LiteralInt(i) => {
                let lit = i.to_string();
                for v in &mut variants {
                    v.push_str(&lit);
                }
            }
            TypeKind::LiteralBool(b) => {
                let lit = b.to_string();
                for v in &mut variants {
                    v.push_str(&lit);
                }
            }
            TypeKind::Union(members) => {
                let mut additions: Vec<String> = vec![];
                for m in members {
                    let lit = match &m.0 {
                        TypeKind::LiteralStr(s) => s.to_string(),
                        TypeKind::LiteralInt(i) => i.to_string(),
                        TypeKind::LiteralBool(b) => b.to_string(),
                        _ => return None,
                    };
                    additions.push(lit);
                }
                let mut next = vec![];
                for existing in &variants {
                    for add in &additions {
                        next.push(format!("{existing}{add}"));
                    }
                }
                variants = next;
            }
            _ => return None,
        }
    }
    Some(variants)
}

pub(super) fn resolve_template_literal_type(
    parts: &[TypeNode],
    ctx: Option<&dyn TypeContext>,
) -> Type {
    let resolved_parts: Vec<Type> = parts.iter().map(|p| resolve_type_node(p, ctx)).collect();
    if let Some(variants) = collect_template_literal_variants(&resolved_parts) {
        let literal_types: Vec<Type> = variants.into_iter().map(Type::literal_str).collect();
        return match literal_types.len() {
            0 => Type::literal_str(""),
            1 => literal_types.into_iter().next().unwrap(),
            _ => Type::union(literal_types),
        };
    }
    Type(TypeKind::TemplateLiteral(resolved_parts), false)
}
