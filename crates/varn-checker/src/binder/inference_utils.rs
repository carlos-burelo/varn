use std::sync::Arc;
use varn_core::ast::{Param, Pattern, TypeNode};

use super::type_resolution::resolve_type_node;
use crate::types::{CheckerTyTable, FunctionParam, FunctionType, ObjectTypeMember, Type};
use varn_core::TypeKind;

pub fn build_fn_type(
    params: &[Param],
    return_type: &Option<TypeNode>,
    is_arrow: bool,
    ctx: Option<&dyn crate::types::TypeContext>,
    table: &mut CheckerTyTable,
    inferred_ret: Type,
) -> Type {
    let interner = ctx.and_then(|c| c.interner());
    let ps = params
        .iter()
        .map(|p| {
            let name = pattern_to_rc_str(&p.pattern, interner);
            let mut ty = p
                .type_ann
                .as_ref()
                .or(match &p.pattern {
                    Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                    _ => None,
                })
                .map(|m| resolve_type_node(m, ctx, table))
                .unwrap_or(Type::Dynamic);
            if p.is_rest && !matches!(table.get(ty.0), TypeKind::Array(_)) {
                ty = Type::array(ty, table);
            }
            FunctionParam {
                name: Some(name),
                ty: ty.0,
                optional: p.is_optional || p.default.is_some(),
                is_rest: p.is_rest,
            }
        })
        .collect();
    let ret = return_type
        .as_ref()
        .map(|m| resolve_type_node(m, ctx, table))
        .unwrap_or(inferred_ret);
    Type::fn_(
        FunctionType {
            params: ps,
            return_type: ret.0,
            is_arrow,
            type_params: vec![],
        },
        table,
    )
}

pub fn build_method_params(
    params: &[Param],
    ctx: Option<&dyn crate::types::TypeContext>,
    table: &mut CheckerTyTable,
) -> Vec<FunctionParam> {
    let interner = ctx.and_then(|c| c.interner());
    params
        .iter()
        .map(|p| {
            let name = pattern_to_rc_str(&p.pattern, interner);
            let mut ty = p
                .type_ann
                .as_ref()
                .map(|m| resolve_type_node(m, ctx, table))
                .unwrap_or(Type::Dynamic);
            if p.is_rest && !matches!(table.get(ty.0), TypeKind::Array(_)) {
                ty = Type::array(ty, table);
            }
            FunctionParam {
                name: Some(name),
                ty: ty.0,
                optional: p.is_optional || p.default.is_some(),
                is_rest: p.is_rest,
            }
        })
        .collect()
}

pub fn infer_object_member_type(
    m: &ObjectTypeMember,
    prop_name: &str,
    table: &mut CheckerTyTable,
) -> Option<Type> {
    match m {
        ObjectTypeMember::Property { name, ty, .. } if name.as_ref() == prop_name => {
            Some(Type(*ty, false))
        }
        ObjectTypeMember::Method {
            name,
            params,
            return_type,
            is_arrow,
            ..
        } if name.as_ref() == prop_name => Some(Type::fn_(
            FunctionType {
                params: params.clone(),
                return_type: *return_type,
                is_arrow: *is_arrow,
                type_params: vec![],
            },
            table,
        )),
        _ => None,
    }
}

/// Renders a synthetic display name for a pattern (used for function-type
/// parameter names / diagnostics). `interner` is `None` only for the rare
/// caller with no `TypeContext` on hand; identifiers then fall back to `_`
/// rather than panicking on an unresolved `Atom`.
pub fn pattern_to_rc_str(p: &Pattern, interner: Option<&varn_core::AtomInterner>) -> Arc<str> {
    match p {
        Pattern::Identifier { name, .. } => match interner {
            Some(i) => Arc::from(i.resolve(*name)),
            None => Arc::from("_"),
        },
        Pattern::Array { elements, rest, .. } => {
            let mut s = "[".to_owned();
            for (i, el) in elements.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                if let Some(el) = el {
                    s.push_str(&pattern_to_rc_str(&el.pattern, interner));
                }
            }
            if let Some(r) = rest {
                if !elements.is_empty() {
                    s.push_str(", ");
                }
                s.push_str("...");
                s.push_str(&pattern_to_rc_str(r, interner));
            }
            s.push(']');
            Arc::from(s)
        }
        Pattern::Object {
            properties, rest, ..
        } => {
            let mut s = "{".to_owned();
            for (i, p) in properties.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                let key_str = match interner {
                    Some(intr) => intr.resolve(p.key).to_owned(),
                    None => "_".to_owned(),
                };
                if p.shorthand {
                    s.push_str(&key_str);
                } else {
                    s.push_str(&format!(
                        "{}: {}",
                        key_str,
                        pattern_to_rc_str(&p.value, interner)
                    ));
                }
            }
            if let Some(r) = rest {
                if !properties.is_empty() {
                    s.push_str(", ");
                }
                s.push_str("...");
                s.push_str(&pattern_to_rc_str(r, interner));
            }
            s.push('}');
            Arc::from(s)
        }
        Pattern::Assignment { left, .. } => pattern_to_rc_str(left, interner),
        Pattern::Rest { argument, .. } => {
            Arc::from(format!("...{}", pattern_to_rc_str(argument, interner)))
        }
    }
}

pub fn pattern_to_string(p: &Pattern, interner: Option<&varn_core::AtomInterner>) -> String {
    pattern_to_rc_str(p, interner).to_string()
}

pub fn widen_literal(ty: Type) -> Type {
    ty
}

pub fn pattern_lead_name<'a>(p: &Pattern, interner: &'a varn_core::AtomInterner) -> &'a str {
    match p {
        Pattern::Identifier { name, .. } => interner.resolve(*name),
        Pattern::Array { .. } => "<array>",
        Pattern::Object { .. } => "<object>",
        Pattern::Rest { .. } => "<rest>",
        Pattern::Assignment { left, .. } => pattern_lead_name(left, interner),
    }
}
