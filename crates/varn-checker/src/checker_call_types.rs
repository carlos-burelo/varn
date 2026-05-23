use crate::binder::resolve_type_node;
use crate::types::{Type, TypeContext};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::ast::{Expr, ExprKind};
use varn_core::IntrinsicType;
use varn_core::TypeKind;

pub(crate) fn infer_call_type(
    fn_map: &FxHashMap<Rc<str>, Type>,
    fn_type_params: &FxHashMap<Rc<str>, Vec<Rc<str>>>,
    class_methods: &FxHashMap<Rc<str>, FxHashMap<Rc<str>, Type>>,
    sym_map: &FxHashMap<Rc<str>, Type>,
    expr: &Expr,
    ctx: Option<&dyn TypeContext>,
    current_class: Option<&str>,
) -> Option<Type> {
    match &expr.kind {
        ExprKind::IntLiteral { .. } => Some(Type::Int),
        ExprKind::FloatLiteral { .. } => Some(Type::Float),
        ExprKind::StrLiteral { .. } => Some(Type::Str),
        ExprKind::BoolLiteral { .. } => Some(Type::Bool),
        ExprKind::This => current_class.map(|n| {
            let origin = ctx.and_then(|c| c.source_file());
            Type::named_with_origin(Rc::from(n), origin.map(|s| Rc::from(s)))
        }),
        ExprKind::Identifier { name } => sym_map.get(name.as_ref()).cloned(),

        ExprKind::Member {
            object,
            property,
            computed: false,
            ..
        } => {
            let prop_name = match &property.kind {
                ExprKind::Identifier { name } => name.as_ref(),
                _ => return None,
            };
            let obj_ty = infer_call_type(
                fn_map,
                fn_type_params,
                class_methods,
                sym_map,
                object,
                ctx,
                current_class,
            )?;
            let (class_name, origin): (&str, Option<&str>) = match &obj_ty.0 {
                TypeKind::Named(n, origin) => (n.as_ref(), origin.as_deref()),
                TypeKind::Generic(name, _, origin) => (name.as_ref(), origin.as_deref()),
                _ => (obj_ty.stdlib_key()?, None),
            };

            if let Some(ctx) = ctx {
                if let Some(members) = ctx.get_class_members(class_name, origin) {
                    if let Some(m) = members.iter().find(|m| m.name.as_ref() == prop_name) {
                        return Some(m.ty.clone());
                    }
                }
            }
            None
        }

        ExprKind::Binary { left, right, op } => {
            let l = infer_call_type(
                fn_map,
                fn_type_params,
                class_methods,
                sym_map,
                left,
                ctx,
                current_class,
            )?;
            let r = infer_call_type(
                fn_map,
                fn_type_params,
                class_methods,
                sym_map,
                right,
                ctx,
                current_class,
            )?;

            use varn_core::ast::operators::BinaryOp;
            match op {
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                    if l == Type::Float || r == Type::Float {
                        Some(Type::Float)
                    } else {
                        Some(Type::Int)
                    }
                }
                BinaryOp::Lt | BinaryOp::Gt | BinaryOp::LtEq | BinaryOp::GtEq => Some(Type::Bool),
                _ => Some(Type::Dynamic),
            }
        }

        ExprKind::Call {
            callee, type_args, ..
        } => {
            let callee_name = match &callee.kind {
                ExprKind::Identifier { name } => Some(name.clone()),
                ExprKind::Member {
                    property,
                    computed: false,
                    ..
                } => match &property.kind {
                    ExprKind::Identifier { name } => Some(name.clone()),
                    _ => None,
                },
                _ => None,
            };

            if let Some(callee_name) = callee_name {
                if let Some(ty) = fn_map.get(callee_name.as_ref() as &str) {
                    if let Some(tps) = fn_type_params.get(callee_name.as_ref() as &str) {
                        let mut mapping = FxHashMap::default();
                        for (i, tp) in tps.iter().enumerate() {
                            if let Some(node) = type_args.get(i) {
                                mapping.insert(tp.clone(), resolve_type_node(node, ctx));
                            }
                        }
                        return Some(ty.map_generics(&mapping));
                    }
                    return Some(ty.clone());
                }
            }

            let callee_ty = infer_call_type(
                fn_map,
                fn_type_params,
                class_methods,
                sym_map,
                callee,
                ctx,
                current_class,
            )?;
            match &callee_ty.0 {
                TypeKind::Fn(ft) => Some((*ft.return_type).clone()),
                _ => None,
            }
        }

        ExprKind::New {
            callee, type_args, ..
        } => {
            if let ExprKind::Identifier { name } = &callee.kind {
                if !type_args.is_empty() {
                    let mut args = Vec::new();
                    for node in type_args {
                        args.push(resolve_type_node(node, ctx));
                    }
                    return Some(Type::generic(name.clone(), args));
                }
                return Some(Type::named(name.clone()));
            }
            None
        }

        ExprKind::Paren { expression } => infer_call_type(
            fn_map,
            fn_type_params,
            class_methods,
            sym_map,
            expression,
            ctx,
            current_class,
        ),

        ExprKind::As { type_ann, .. } => Some(resolve_type_node(type_ann, ctx)),

        ExprKind::Await { argument } => {
            let ty = infer_call_type(
                fn_map,
                fn_type_params,
                class_methods,
                sym_map,
                argument,
                ctx,
                current_class,
            )?;
            match &ty.0 {
                TypeKind::Generic(name, args, _)
                    if (name.as_ref() == IntrinsicType::Task.as_str()
                        || name.as_ref() == IntrinsicType::TaskHandle.as_str())
                        && args.len() == 1 =>
                {
                    Some(args[0].clone())
                }
                _ => Some(ty),
            }
        }

        ExprKind::Conditional {
            consequent,
            alternate,
            ..
        } => {
            let t = infer_call_type(
                fn_map,
                fn_type_params,
                class_methods,
                sym_map,
                consequent,
                ctx,
                current_class,
            )?;
            let f = infer_call_type(
                fn_map,
                fn_type_params,
                class_methods,
                sym_map,
                alternate,
                ctx,
                current_class,
            )?;
            if t == f {
                Some(t)
            } else {
                Some(Type::union(vec![t, f]))
            }
        }

        ExprKind::Function {
            params,
            return_type,
            ..
        } => Some(crate::binder::build_fn_type(
            params,
            return_type,
            false,
            ctx,
            Type::Dynamic,
        )),

        ExprKind::Arrow {
            params,
            return_type,
            body,
            ..
        } => {
            use varn_core::ast::ArrowBody;
            let inferred_ret = if let ArrowBody::Expr(e) = body.as_ref() {
                infer_call_type(
                    fn_map,
                    fn_type_params,
                    class_methods,
                    sym_map,
                    e,
                    ctx,
                    current_class,
                )
                .unwrap_or(Type::Dynamic)
            } else {
                Type::Dynamic
            };
            Some(crate::binder::build_fn_type(
                params,
                return_type,
                true,
                ctx,
                inferred_ret,
            ))
        }

        ExprKind::Match { cases, .. } => {
            let mut tys = Vec::new();
            for case in cases {
                match &case.body {
                    varn_core::ast::MatchBody::Expr(e) => {
                        if let Some(ty) = infer_call_type(
                            fn_map,
                            fn_type_params,
                            class_methods,
                            sym_map,
                            e,
                            ctx,
                            current_class,
                        ) {
                            tys.push(ty);
                        }
                    }
                    varn_core::ast::MatchBody::Block(_) => {
                        tys.push(Type::Void);
                    }
                }
            }
            if tys.is_empty() {
                Some(Type::Dynamic)
            } else {
                let first = tys[0].clone();
                if tys.iter().all(|t| t == &first) {
                    Some(first)
                } else {
                    Some(Type::union(tys))
                }
            }
        }

        _ => Some(Type::Dynamic),
    }
}
