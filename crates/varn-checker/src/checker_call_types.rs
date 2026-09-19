use crate::binder::resolve_type_node;
use crate::types::{CheckerTyTable, Type, TypeContext};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::ast::{AstArena, ExprId, ExprKind};
use varn_core::AtomInterner;
use varn_core::IntrinsicType;
use varn_core::TypeKind;

#[allow(clippy::too_many_arguments)]
pub(crate) fn infer_call_type(
    fn_map: &FxHashMap<Rc<str>, Type>,
    fn_type_params: &FxHashMap<Rc<str>, Vec<Rc<str>>>,
    class_methods: &FxHashMap<Rc<str>, FxHashMap<Rc<str>, Type>>,
    sym_map: &FxHashMap<Rc<str>, Type>,
    expr: ExprId,
    ast_arena: &AstArena,
    ctx: Option<&dyn TypeContext>,
    current_class: Option<&str>,
    interner: &AtomInterner,
    table: &mut CheckerTyTable,
) -> Option<Type> {
    match &ast_arena.expr(expr).kind {
        ExprKind::IntLiteral { .. } => Some(Type::Int),
        ExprKind::FloatLiteral { .. } => Some(Type::Float),
        ExprKind::StrLiteral { .. } => Some(Type::Str),
        ExprKind::BoolLiteral { .. } => Some(Type::Bool),
        ExprKind::This => current_class.and_then(|n| {
            let resolver = ctx.and_then(|c| c.resolver())?;
            let origin = ctx.and_then(|c| c.source_file());
            Some(Type::named_with_origin(
                Rc::from(n),
                origin.map(Rc::from),
                resolver,
                table,
            ))
        }),
        ExprKind::Identifier { name } => sym_map.get(interner.resolve(*name)).cloned(),

        ExprKind::Member {
            object,
            property,
            computed: false,
            ..
        } => {
            let (object, property) = (*object, *property);
            let prop_name = match &ast_arena.expr(property).kind {
                ExprKind::Identifier { name } => interner.resolve(*name),
                _ => return None,
            };
            let obj_ty = infer_call_type(
                fn_map,
                fn_type_params,
                class_methods,
                sym_map,
                object,
                ast_arena,
                ctx,
                current_class,
                interner,
                table,
            )?;
            let obj_kind = *table.get(obj_ty.0);
            let (class_name, origin): (&str, Option<&str>) = match obj_kind {
                TypeKind::Named(n, origin) => (
                    interner.resolve(n),
                    origin.map(|o| interner.resolve(o)),
                ),
                TypeKind::Generic(name, _, origin) => (
                    interner.resolve(name),
                    origin.map(|o| interner.resolve(o)),
                ),
                _ => (obj_ty.stdlib_key(table)?, None),
            };

            if let Some(ctx) = ctx {
                if let Some(members) = ctx.get_class_members(class_name, origin) {
                    if let Some(m) = members.iter().find(|m| m.name.as_ref() == prop_name) {
                        return Some(m.ty.clone());
                    }
                }
                if let Some(ext_ty) = ctx.get_extension_method(class_name, prop_name) {
                    return Some(ext_ty);
                }
            }
            if let Some(methods) = class_methods.get(class_name) {
                if let Some(ty) = methods.get(prop_name) {
                    return Some(ty.clone());
                }
            }
            None
        }

        ExprKind::Binary { left, right, op } => {
            let (left, right, op) = (*left, *right, *op);
            let l = infer_call_type(
                fn_map,
                fn_type_params,
                class_methods,
                sym_map,
                left,
                ast_arena,
                ctx,
                current_class,
                interner,
                table,
            )?;
            let r = infer_call_type(
                fn_map,
                fn_type_params,
                class_methods,
                sym_map,
                right,
                ast_arena,
                ctx,
                current_class,
                interner,
                table,
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
            let callee = *callee;
            let callee_name = match &ast_arena.expr(callee).kind {
                ExprKind::Identifier { name } => Some(*name),
                ExprKind::Member {
                    property,
                    computed: false,
                    ..
                } => match &ast_arena.expr(*property).kind {
                    ExprKind::Identifier { name } => Some(*name),
                    _ => None,
                },
                _ => None,
            };

            if let Some(callee_name) = callee_name {
                let callee_name_str = interner.resolve(callee_name);
                if let Some(ty) = fn_map.get(callee_name_str) {
                    if let Some(tps) = fn_type_params.get(callee_name_str) {
                        let mut mapping: FxHashMap<varn_core::Atom, Type> = FxHashMap::default();
                        if let Some(resolver) = ctx.and_then(|c| c.resolver()) {
                            for (i, tp) in tps.iter().enumerate() {
                                if let Some(node) = type_args.get(i) {
                                    let resolved = resolve_type_node(node, ctx, table);
                                    mapping.insert(resolver.intern(tp), resolved);
                                }
                            }
                        }
                        return Some(ty.map_generics(&mapping, table));
                    }
                    return Some(*ty);
                }
            }

            let callee_ty = infer_call_type(
                fn_map,
                fn_type_params,
                class_methods,
                sym_map,
                callee,
                ast_arena,
                ctx,
                current_class,
                interner,
                table,
            )?;
            match *table.get(callee_ty.0) {
                TypeKind::Fn(fid) => Some(Type(table.get_function(fid).return_type, false)),
                _ => None,
            }
        }

        ExprKind::New {
            callee, type_args, ..
        } => {
            if let ExprKind::Identifier { name } = &ast_arena.expr(*callee).kind {
                let resolver = ctx.and_then(|c| c.resolver())?;
                let name_str = interner.resolve(*name);
                if !type_args.is_empty() {
                    let mut args = Vec::new();
                    for node in type_args {
                        args.push(resolve_type_node(node, ctx, table));
                    }
                    return Some(Type::generic(Rc::from(name_str), args, resolver, table));
                }
                if name_str == IntrinsicType::Map.as_str() {
                    return Some(Type::generic(
                        Rc::from(name_str),
                        vec![Type::Dynamic],
                        resolver,
                        table,
                    ));
                }
                return Some(Type::named(Rc::from(name_str), resolver, table));
            }
            None
        }

        ExprKind::Paren { expression } => infer_call_type(
            fn_map,
            fn_type_params,
            class_methods,
            sym_map,
            *expression,
            ast_arena,
            ctx,
            current_class,
            interner,
            table,
        ),

        ExprKind::As { type_ann, .. } => Some(resolve_type_node(type_ann, ctx, table)),

        ExprKind::Await { argument } => {
            let ty = infer_call_type(
                fn_map,
                fn_type_params,
                class_methods,
                sym_map,
                *argument,
                ast_arena,
                ctx,
                current_class,
                interner,
                table,
            )?;
            match *table.get(ty.0) {
                TypeKind::Generic(name, args, _)
                    if (interner.resolve(name) == IntrinsicType::Task.as_str()
                        || interner.resolve(name) == IntrinsicType::TaskHandle.as_str())
                        && table.get_list(args).len() == 1 =>
                {
                    Some(Type(table.get_list(args)[0], false))
                }
                _ => Some(ty),
            }
        }

        ExprKind::Conditional {
            consequent,
            alternate,
            ..
        } => {
            let (consequent, alternate) = (*consequent, *alternate);
            let t = infer_call_type(
                fn_map,
                fn_type_params,
                class_methods,
                sym_map,
                consequent,
                ast_arena,
                ctx,
                current_class,
                interner,
                table,
            )?;
            let f = infer_call_type(
                fn_map,
                fn_type_params,
                class_methods,
                sym_map,
                alternate,
                ast_arena,
                ctx,
                current_class,
                interner,
                table,
            )?;
            if t == f {
                Some(t)
            } else {
                Some(Type::union(vec![t, f], table))
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
            table,
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
                    *e,
                    ast_arena,
                    ctx,
                    current_class,
                    interner,
                    table,
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
                table,
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
                            *e,
                            ast_arena,
                            ctx,
                            current_class,
                            interner,
                            table,
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
                let first = tys[0];
                if tys.iter().all(|t| t == &first) {
                    Some(first)
                } else {
                    Some(Type::union(tys, table))
                }
            }
        }

        ExprKind::Pipeline { right, .. } => infer_call_type(
            fn_map,
            fn_type_params,
            class_methods,
            sym_map,
            *right,
            ast_arena,
            ctx,
            current_class,
            interner,
            table,
        ),

        _ => Some(Type::Dynamic),
    }
}
