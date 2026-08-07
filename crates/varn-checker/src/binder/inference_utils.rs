use std::rc::Rc;
use varn_core::ast::{Param, Pattern, TypeNode};

use super::type_resolution::resolve_type_node;
use crate::types::{FunctionParam, FunctionType, ObjectTypeMember, Type};
use varn_core::TypeKind;

pub fn build_fn_type(
    params: &[Param],
    return_type: &Option<TypeNode>,
    is_arrow: bool,
    ctx: Option<&dyn crate::types::TypeContext>,
    inferred_ret: Type,
) -> Type {
    let ps = params
        .iter()
        .map(|p| {
            let name = pattern_to_rc_str(&p.pattern);
            let mut ty = p
                .type_ann
                .as_ref()
                .or(match &p.pattern {
                    Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                    _ => None,
                })
                .map(|m| resolve_type_node(m, ctx))
                .unwrap_or(Type::Dynamic);
            if p.is_rest && !matches!(ty.0, TypeKind::Array(_)) {
                ty = Type::array(ty);
            }
            FunctionParam {
                name: Some(name),
                ty,
                optional: p.is_optional || p.default.is_some(),
                is_rest: p.is_rest,
            }
        })
        .collect();
    let ret = return_type
        .as_ref()
        .map(|m| resolve_type_node(m, ctx))
        .unwrap_or(inferred_ret);
    Type::fn_(FunctionType {
        params: ps,
        return_type: Box::new(ret),
        is_arrow,
        type_params: vec![],
    })
}

pub fn build_method_params(
    params: &[Param],
    ctx: Option<&dyn crate::types::TypeContext>,
) -> Vec<FunctionParam> {
    params
        .iter()
        .map(|p| {
            let name = pattern_to_rc_str(&p.pattern);
            let mut ty = p
                .type_ann
                .as_ref()
                .map(|m| resolve_type_node(m, ctx))
                .unwrap_or(Type::Dynamic);
            if p.is_rest && !matches!(ty.0, TypeKind::Array(_)) {
                ty = Type::array(ty);
            }
            FunctionParam {
                name: Some(name),
                ty,
                optional: p.is_optional || p.default.is_some(),
                is_rest: p.is_rest,
            }
        })
        .collect()
}

pub fn infer_object_member_type(m: &ObjectTypeMember, prop_name: &str) -> Option<Type> {
    match m {
        ObjectTypeMember::Property { name, ty, .. } if name.as_ref() == prop_name => {
            Some(ty.clone())
        }
        ObjectTypeMember::Method {
            name,
            params,
            return_type,
            is_arrow,
            ..
        } if name.as_ref() == prop_name => Some(Type::fn_(FunctionType {
            params: params.clone(),
            return_type: return_type.clone(),
            is_arrow: *is_arrow,
            type_params: vec![],
        })),
        _ => None,
    }
}

pub fn pattern_to_rc_str(p: &Pattern) -> Rc<str> {
    match p {
        Pattern::Identifier { name, .. } => name.clone(),
        Pattern::Array { elements, rest, .. } => {
            let mut s = "[".to_owned();
            for (i, el) in elements.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                if let Some(el) = el {
                    s.push_str(&pattern_to_rc_str(&el.pattern));
                }
            }
            if let Some(r) = rest {
                if !elements.is_empty() {
                    s.push_str(", ");
                }
                s.push_str("...");
                s.push_str(&pattern_to_rc_str(r));
            }
            s.push(']');
            Rc::from(s)
        }
        Pattern::Object {
            properties, rest, ..
        } => {
            let mut s = "{".to_owned();
            for (i, p) in properties.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                if p.shorthand {
                    s.push_str(&p.key);
                } else {
                    s.push_str(&format!("{}: {}", p.key, pattern_to_rc_str(&p.value)));
                }
            }
            if let Some(r) = rest {
                if !properties.is_empty() {
                    s.push_str(", ");
                }
                s.push_str("...");
                s.push_str(&pattern_to_rc_str(r));
            }
            s.push('}');
            Rc::from(s)
        }
        Pattern::Assignment { left, .. } => pattern_to_rc_str(left),
        Pattern::Rest { argument, .. } => Rc::from(format!("...{}", pattern_to_rc_str(argument))),
    }
}

pub fn pattern_to_string(p: &Pattern) -> String {
    pattern_to_rc_str(p).to_string()
}

pub fn widen_literal(ty: Type) -> Type {
    ty
}

pub fn pattern_lead_name(p: &Pattern) -> &str {
    match p {
        Pattern::Identifier { name, .. } => name,
        Pattern::Array { .. } => "<array>",
        Pattern::Object { .. } => "<object>",
        Pattern::Rest { .. } => "<rest>",
        Pattern::Assignment { left, .. } => pattern_lead_name(left),
    }
}
