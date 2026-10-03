use crate::types::{CheckerTyTable, ObjectTypeMember, Type, TypeContext};
use std::sync::Arc;
use varn_core::ast::InterfaceMember;

use super::names::resolve_atom_name;
use super::resolve_type_node;

pub(super) fn resolve_object_type(
    members: &[InterfaceMember],
    ctx: Option<&dyn TypeContext>,
    interner: &varn_core::AtomInterner,
    table: &mut CheckerTyTable,
) -> Type {
    let resolved_members: Vec<ObjectTypeMember> = members
        .iter()
        .map(|m| match m {
            InterfaceMember::Property {
                key,
                type_ann,
                optional,
                readonly,
                ..
            } => {
                let ty = resolve_type_node(type_ann, ctx, table);
                ObjectTypeMember::Property {
                    name: resolve_atom_name(*key, ctx, interner),
                    ty: ty.0,
                    optional: *optional,
                    readonly: *readonly,
                }
            }
            InterfaceMember::Method {
                key,
                params,
                return_type,
                optional,
                is_async,
                ..
            } => {
                let resolved_params = params
                    .iter()
                    .map(|p| {
                        let mut ty = p
                            .type_ann
                            .as_ref()
                            .or(match &p.pattern {
                                varn_core::ast::Pattern::Identifier { type_ann, .. } => {
                                    type_ann.as_ref()
                                }
                                _ => None,
                            })
                            .map(|ann| resolve_type_node(ann, ctx, table))
                            .unwrap_or(Type::Dynamic);
                        if p.is_rest && !matches!(table.get(ty.0), varn_core::TypeKind::Array(_)) {
                            ty = Type::array(ty, table);
                        }
                        crate::types::FunctionParam {
                            name: Some(Arc::from(crate::binder::pattern_lead_name(
                                &p.pattern, interner,
                            ))),
                            ty: ty.0,
                            optional: p.is_optional || p.default.is_some(),
                            is_rest: p.is_rest,
                        }
                    })
                    .collect::<Vec<_>>();

                let ret_resolved = return_type
                    .as_ref()
                    .map(|m| resolve_type_node(m, ctx, table))
                    .unwrap_or(Type::Dynamic);
                let ret = crate::types::async_fn_return(ret_resolved, *is_async, table, interner);
                ObjectTypeMember::Method {
                    name: resolve_atom_name(*key, ctx, interner),
                    params: resolved_params,
                    return_type: ret.0,
                    optional: *optional,
                    is_arrow: false,
                }
            }
            InterfaceMember::Index {
                param, return_type, ..
            } => {
                let key_ty = param
                    .type_ann
                    .as_ref()
                    .or(match &param.pattern {
                        varn_core::ast::Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                        _ => None,
                    })
                    .map(|ann| resolve_type_node(ann, ctx, table))
                    .unwrap_or(Type::Str);
                let value_ty = resolve_type_node(return_type, ctx, table);
                ObjectTypeMember::Index {
                    param_name: Arc::from(crate::binder::pattern_lead_name(
                        &param.pattern,
                        interner,
                    )),
                    key_ty: key_ty.0,
                    value_ty: value_ty.0,
                }
            }
            InterfaceMember::Callable {
                params,
                return_type,
                ..
            } => {
                let resolved_params = params
                    .iter()
                    .map(|p| {
                        let mut ty = p
                            .type_ann
                            .as_ref()
                            .or(match &p.pattern {
                                varn_core::ast::Pattern::Identifier { type_ann, .. } => {
                                    type_ann.as_ref()
                                }
                                _ => None,
                            })
                            .map(|ann| resolve_type_node(ann, ctx, table))
                            .unwrap_or(Type::Dynamic);
                        if p.is_rest && !matches!(table.get(ty.0), varn_core::TypeKind::Array(_)) {
                            ty = Type::array(ty, table);
                        }
                        crate::types::FunctionParam {
                            name: Some(Arc::from(crate::binder::pattern_lead_name(
                                &p.pattern, interner,
                            ))),
                            ty: ty.0,
                            optional: p.is_optional || p.default.is_some(),
                            is_rest: p.is_rest,
                        }
                    })
                    .collect::<Vec<_>>();
                let ret_ty = resolve_type_node(return_type, ctx, table);
                ObjectTypeMember::Callable {
                    params: resolved_params,
                    return_type: ret_ty.0,
                    is_arrow: false,
                }
            }
        })
        .collect();
    Type::object(resolved_members, table)
}
