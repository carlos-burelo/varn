mod aliases;
mod conditional;
mod contexts;
mod keyed_access;
mod mapped;
mod template;

use crate::types::{FunctionType, ObjectTypeMember, Type, TypeContext};
use std::rc::Rc;
use varn_core::ast::TypeNode;
use varn_core::TypeKind;

use aliases::{is_primitive_type, try_stdlib_generic_alias};
use conditional::resolve_conditional;
use contexts::AliasSubstitutionContext;
use keyed_access::{resolve_indexed_access, resolve_keyof};
use mapped::resolve_mapped;
use template::resolve_template_literal_type;

pub use aliases::resolve_primitive;

pub fn resolve_type_node(node: &TypeNode, ctx: Option<&dyn TypeContext>) -> Type {
    use varn_core::TypeTag;
    match &node.kind {
        TypeKind::Intrinsic(TypeTag::Int) => Type::Int,
        TypeKind::Intrinsic(TypeTag::Float) => Type::Float,
        TypeKind::Intrinsic(TypeTag::Decimal) => Type::Decimal,
        TypeKind::Intrinsic(TypeTag::BigInt) => Type::BigInt,
        TypeKind::Intrinsic(TypeTag::Str) => Type::Str,
        TypeKind::Intrinsic(TypeTag::Char) => Type::Char,
        TypeKind::Intrinsic(TypeTag::Bool) => Type::Bool,
        TypeKind::Intrinsic(TypeTag::Symbol) => Type::Symbol,
        TypeKind::Intrinsic(TypeTag::Void) => Type::Void,
        TypeKind::Intrinsic(TypeTag::Null) => Type::Null,
        TypeKind::Intrinsic(TypeTag::Never) => Type::Never,
        TypeKind::Intrinsic(TypeTag::Dynamic) => Type::Dynamic,
        TypeKind::This => Type::This,

        TypeKind::Array(inner) => Type::array(resolve_type_node(inner, ctx)),
        TypeKind::Union(members) => {
            Type::union(members.iter().map(|m| resolve_type_node(m, ctx)).collect())
        }
        TypeKind::Generic(name, args, _origin) => {
            let resolved_args: Vec<Type> = args.iter().map(|m| resolve_type_node(m, ctx)).collect();

            if let Some((params, alias_node)) = ctx.and_then(|c| c.get_alias_node(name.as_ref())) {
                if !params.is_empty() && params.len() == resolved_args.len() {
                    let alias_ctx = AliasSubstitutionContext {
                        inner: ctx,
                        params,
                        args: resolved_args,
                    };
                    return resolve_type_node(&alias_node, Some(&alias_ctx));
                }
            }

            if let Some(ty) = try_stdlib_generic_alias(name.as_ref(), &resolved_args, ctx) {
                return ty;
            }

            Type::generic_with_origin(
                name.clone(),
                resolved_args,
                ctx.and_then(|c| c.source_file()).map(|s| Rc::from(s)),
            )
        }
        TypeKind::Named(name, __origin) => {
            let prim = resolve_primitive(name.as_ref(), ctx);
            if !matches!(&prim.0, TypeKind::Named(_, _)) {
                return prim;
            }

            if let Some(resolved) = ctx.and_then(|c| c.resolve_symbol(name.as_ref())) {
                return resolved;
            }

            prim
        }
        TypeKind::Fn((params, ret)) => {
            let resolved_params = params
                .iter()
                .map(|p| {
                    let ty = p
                        .constraint
                        .as_ref()
                        .map(|m| resolve_type_node(m, ctx))
                        .unwrap_or(Type::Dynamic);
                    crate::types::FunctionParam {
                        name: Some(Rc::from(p.name.clone())),
                        ty,
                        optional: false,
                        is_rest: false,
                    }
                })
                .collect();
            Type::fn_(FunctionType {
                params: resolved_params,
                return_type: Box::new(resolve_type_node(ret, ctx)),
                is_arrow: false,
                type_params: vec![],
            })
        }
        TypeKind::Object(members) => {
            let resolved_members = members
                .iter()
                .map(|m| match m {
                    varn_core::ast::InterfaceMember::Property {
                        key,
                        type_ann,
                        optional,
                        readonly,
                        ..
                    } => ObjectTypeMember::Property {
                        name: key.clone(),
                        ty: resolve_type_node(type_ann, ctx),
                        optional: *optional,
                        readonly: *readonly,
                    },
                    varn_core::ast::InterfaceMember::Method {
                        key,
                        params,
                        return_type,
                        optional,
                        ..
                    } => {
                        let resolved_params = params
                            .iter()
                            .map(|p| {
                                let mut ty = p
                                    .type_ann
                                    .as_ref()
                                    .or(match &p.pattern {
                                        varn_core::ast::Pattern::Identifier {
                                            type_ann, ..
                                        } => type_ann.as_ref(),
                                        _ => None,
                                    })
                                    .map(|ann| resolve_type_node(ann, ctx))
                                    .unwrap_or(Type::Dynamic);
                                if p.is_rest && !matches!(ty.0, TypeKind::Array(_)) {
                                    ty = Type::array(ty);
                                }
                                crate::types::FunctionParam {
                                    name: Some(Rc::from(crate::binder::pattern_lead_name(
                                        &p.pattern,
                                    ))),
                                    ty,
                                    optional: p.is_optional || p.default.is_some(),
                                    is_rest: p.is_rest,
                                }
                            })
                            .collect::<Vec<_>>();

                        let ret = return_type
                            .as_ref()
                            .map(|m| resolve_type_node(m, ctx))
                            .unwrap_or(Type::Dynamic);
                        ObjectTypeMember::Method {
                            name: key.clone(),
                            params: resolved_params,
                            return_type: Box::new(ret),
                            optional: *optional,
                            is_arrow: false,
                        }
                    }
                    varn_core::ast::InterfaceMember::Index {
                        param, return_type, ..
                    } => {
                        let key_ty = param
                            .type_ann
                            .as_ref()
                            .or(match &param.pattern {
                                varn_core::ast::Pattern::Identifier { type_ann, .. } => {
                                    type_ann.as_ref()
                                }
                                _ => None,
                            })
                            .map(|ann| resolve_type_node(ann, ctx))
                            .unwrap_or(Type::Str);
                        ObjectTypeMember::Index {
                            param_name: Rc::from(crate::binder::pattern_lead_name(&param.pattern)),
                            key_ty: Box::new(key_ty),
                            value_ty: Box::new(resolve_type_node(return_type, ctx)),
                        }
                    }
                    varn_core::ast::InterfaceMember::Callable {
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
                                        varn_core::ast::Pattern::Identifier {
                                            type_ann, ..
                                        } => type_ann.as_ref(),
                                        _ => None,
                                    })
                                    .map(|ann| resolve_type_node(ann, ctx))
                                    .unwrap_or(Type::Dynamic);
                                if p.is_rest && !matches!(ty.0, TypeKind::Array(_)) {
                                    ty = Type::array(ty);
                                }
                                crate::types::FunctionParam {
                                    name: Some(Rc::from(crate::binder::pattern_lead_name(
                                        &p.pattern,
                                    ))),
                                    ty,
                                    optional: p.is_optional || p.default.is_some(),
                                    is_rest: p.is_rest,
                                }
                            })
                            .collect::<Vec<_>>();
                        ObjectTypeMember::Callable {
                            params: resolved_params,
                            return_type: Box::new(resolve_type_node(return_type, ctx)),
                            is_arrow: false,
                        }
                    }
                })
                .collect();
            Type::object(resolved_members)
        }
        TypeKind::TemplateLiteral(parts) => resolve_template_literal_type(parts, ctx),
        TypeKind::LiteralInt(v) => Type::literal_int(*v),
        TypeKind::LiteralFloat(v) => Type::literal_float(*v),
        TypeKind::LiteralStr(v) => Type::literal_str(v.to_string()),
        TypeKind::LiteralBool(v) => Type::literal_bool(*v),

        TypeKind::Typeof(expr) => crate::binder::infer_expr_type(expr, ctx),

        TypeKind::Intersection(members) => {
            let resolved: Vec<Type> = members.iter().map(|m| resolve_type_node(m, ctx)).collect();

            let primitives: Vec<&Type> = resolved.iter().filter(|m| is_primitive_type(m)).collect();
            if primitives.len() > 1 {
                let first = primitives[0];
                let incompatible = primitives
                    .iter()
                    .any(|m| std::mem::discriminant(&m.0) != std::mem::discriminant(&first.0));
                if incompatible {
                    return Type::Never;
                }
            }

            let parts_opt: Option<Vec<Vec<ObjectTypeMember>>> = resolved
                .iter()
                .map(|m| match &m.0 {
                    TypeKind::Object(members) => Some(members.clone()),
                    TypeKind::Named(name, origin) => {
                        let ctx = ctx?;
                        let members = ctx
                            .get_class_members(name.as_ref(), origin.as_deref())
                            .or_else(|| {
                                ctx.get_interface_members(name.as_ref(), origin.as_deref())
                            })?;
                        Some(
                            members
                                .iter()
                                .map(|cm| {
                                    use crate::types::ClassMemberKind;
                                    match cm.kind {
                                        ClassMemberKind::Method => {
                                            if let TypeKind::Fn(ft) = &cm.ty.0 {
                                                ObjectTypeMember::Method {
                                                    name: cm.name.clone(),
                                                    params: ft.params.clone(),
                                                    return_type: ft.return_type.clone(),
                                                    optional: cm.is_optional,
                                                    is_arrow: ft.is_arrow,
                                                }
                                            } else {
                                                ObjectTypeMember::Property {
                                                    name: cm.name.clone(),
                                                    ty: cm.ty.clone(),
                                                    optional: cm.is_optional,
                                                    readonly: cm.is_readonly,
                                                }
                                            }
                                        }
                                        _ => ObjectTypeMember::Property {
                                            name: cm.name.clone(),
                                            ty: cm.ty.clone(),
                                            optional: cm.is_optional,
                                            readonly: cm.is_readonly,
                                        },
                                    }
                                })
                                .collect(),
                        )
                    }
                    _ => None,
                })
                .collect();

            if let Some(parts) = parts_opt {
                return Type::object(parts.into_iter().flatten().collect());
            }
            Type(TypeKind::Intersection(resolved), false)
        }

        TypeKind::KeyOf(inner) => {
            let resolved = resolve_type_node(inner, ctx);
            resolve_keyof(resolved, ctx)
        }

        TypeKind::IndexedAccess { object, index } => {
            let obj = resolve_type_node(object, ctx);
            let idx = resolve_type_node(index, ctx);
            resolve_indexed_access(obj, idx, ctx)
        }

        TypeKind::Mapped {
            key_var,
            source,
            value,
            optional,
            readonly,
        } => {
            let resolved_source = resolve_type_node(source, ctx);

            let source_obj = if !*optional {
                if let TypeKind::KeyOf(inner) = &source.kind {
                    Some(resolve_type_node(inner, ctx))
                } else {
                    None
                }
            } else {
                None
            };
            resolve_mapped(
                key_var,
                resolved_source,
                value,
                *optional,
                *readonly,
                source_obj,
                ctx,
            )
        }

        TypeKind::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            let check_ty = resolve_type_node(check, ctx);
            resolve_conditional(check, &check_ty, extends, true_type, false_type, ctx)
        }

        TypeKind::Infer(_) => Type::Dynamic.tainted(),

        _ => Type::Dynamic.tainted(),
    }
}
