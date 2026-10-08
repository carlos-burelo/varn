use crate::binder::resolve_type_node;
use crate::types::{CheckerTyTable, Type, TypeContext};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::{AstArena, ExprId, ExprKind};
use varn_core::AtomInterner;
use varn_core::TypeKind;
pub(crate) fn infer_call_type(
    fn_map: &FxHashMap<Arc<str>, Type>,
    fn_type_params: &FxHashMap<Arc<str>, Vec<Arc<str>>>,
    class_methods: &FxHashMap<Arc<str>, FxHashMap<Arc<str>, Type>>,
    sym_map: &FxHashMap<Arc<str>, Type>,
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
        ExprKind::This => current_class.map(|n| {
            let origin = ctx.and_then(|c| c.source_file());
            Type::named_with_origin(Arc::from(n), origin.map(Arc::from), table)
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
                ExprKind::IntLiteral { .. }
                | ExprKind::FloatLiteral { .. }
                | ExprKind::BigIntLiteral { .. }
                | ExprKind::DecimalLiteral { .. }
                | ExprKind::StrLiteral { .. }
                | ExprKind::CharLiteral { .. }
                | ExprKind::BoolLiteral { .. }
                | ExprKind::NullLiteral
                | ExprKind::RegexLiteral { .. }
                | ExprKind::Template { .. }
                | ExprKind::TaggedTemplate { .. }
                | ExprKind::Missing
                | ExprKind::This
                | ExprKind::Super
                | ExprKind::Array { .. }
                | ExprKind::Object { .. }
                | ExprKind::Tuple { .. }
                | ExprKind::Record { .. }
                | ExprKind::Unary { .. }
                | ExprKind::Update { .. }
                | ExprKind::Binary { .. }
                | ExprKind::Logical { .. }
                | ExprKind::Assign { .. }
                | ExprKind::Conditional { .. }
                | ExprKind::Member { .. }
                | ExprKind::Call { .. }
                | ExprKind::New { .. }
                | ExprKind::Function { .. }
                | ExprKind::Arrow { .. }
                | ExprKind::Sequence { .. }
                | ExprKind::Paren { .. }
                | ExprKind::Await { .. }
                | ExprKind::Spawn { .. }
                | ExprKind::Yield { .. }
                | ExprKind::Spread { .. }
                | ExprKind::Pipeline { .. }
                | ExprKind::Range { .. }
                | ExprKind::NonNull { .. }
                | ExprKind::Try { .. }
                | ExprKind::As { .. }
                | ExprKind::Satisfies { .. }
                | ExprKind::ClassExpr { .. }
                | ExprKind::Match { .. }
                | ExprKind::Is { .. }
                | ExprKind::With { .. }
                | ExprKind::MetaAccess { .. } => return None,
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
            let obj_kind = table.get(obj_ty.0);
            let text_of = |atom| -> Option<String> {
                ctx.and_then(|c| c.atom_text(atom))
                    .or_else(|| table.name(atom).map(str::to_owned))
            };
            let (class_name, origin): (Option<String>, Option<String>) = match obj_kind {
                TypeKind::Named(n, origin) => (text_of(n), origin.and_then(|o| text_of(o))),
                TypeKind::Generic(name, _, origin) => {
                    (text_of(name), origin.and_then(|o| text_of(o)))
                }
                TypeKind::Primitive(_)
                | TypeKind::Builtin(_)
                | TypeKind::Literal(_)
                | TypeKind::This
                | TypeKind::Array(_)
                | TypeKind::Union(_)
                | TypeKind::Intersection(_)
                | TypeKind::Tuple(_)
                | TypeKind::TemplateLiteral(_)
                | TypeKind::Fn(_)
                | TypeKind::Object(_)
                | TypeKind::Typeof(_)
                | TypeKind::KeyOf(_)
                | TypeKind::IndexedAccess { .. }
                | TypeKind::Mapped { .. }
                | TypeKind::Conditional { .. }
                | TypeKind::Infer(_)
                | TypeKind::EnumVariant { .. }
                | TypeKind::TypePredicate { .. } => {
                    (Some(obj_ty.stdlib_key(table)?.to_string()), None)
                }
            };
            let (Some(class_name), origin) = (class_name, origin) else {
                return None;
            };

            if let Some(ctx) = ctx {
                if let Some(members) = ctx.get_class_members(&class_name, origin.as_deref()) {
                    if let Some(m) = members.iter().find(|m| m.name.as_ref() == prop_name) {
                        return Some(m.ty);
                    }
                }
                if let Some(ext_ty) = ctx.get_extension_method(&class_name, prop_name) {
                    return Some(ext_ty);
                }
            }
            if let Some(methods) = class_methods.get(class_name.as_str()) {
                if let Some(ty) = methods.get(prop_name) {
                    return Some(*ty);
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
                BinaryOp::Mod
                | BinaryOp::Pow
                | BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr
                | BinaryOp::UShr
                | BinaryOp::Instanceof
                | BinaryOp::In => Some(Type::Dynamic),
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
                    ExprKind::IntLiteral { .. }
                    | ExprKind::FloatLiteral { .. }
                    | ExprKind::BigIntLiteral { .. }
                    | ExprKind::DecimalLiteral { .. }
                    | ExprKind::StrLiteral { .. }
                    | ExprKind::CharLiteral { .. }
                    | ExprKind::BoolLiteral { .. }
                    | ExprKind::NullLiteral
                    | ExprKind::RegexLiteral { .. }
                    | ExprKind::Template { .. }
                    | ExprKind::TaggedTemplate { .. }
                    | ExprKind::Missing
                    | ExprKind::This
                    | ExprKind::Super
                    | ExprKind::Array { .. }
                    | ExprKind::Object { .. }
                    | ExprKind::Tuple { .. }
                    | ExprKind::Record { .. }
                    | ExprKind::Unary { .. }
                    | ExprKind::Update { .. }
                    | ExprKind::Binary { .. }
                    | ExprKind::Logical { .. }
                    | ExprKind::Assign { .. }
                    | ExprKind::Conditional { .. }
                    | ExprKind::Member { .. }
                    | ExprKind::Call { .. }
                    | ExprKind::New { .. }
                    | ExprKind::Function { .. }
                    | ExprKind::Arrow { .. }
                    | ExprKind::Sequence { .. }
                    | ExprKind::Paren { .. }
                    | ExprKind::Await { .. }
                    | ExprKind::Spawn { .. }
                    | ExprKind::Yield { .. }
                    | ExprKind::Spread { .. }
                    | ExprKind::Pipeline { .. }
                    | ExprKind::Range { .. }
                    | ExprKind::NonNull { .. }
                    | ExprKind::Try { .. }
                    | ExprKind::As { .. }
                    | ExprKind::Satisfies { .. }
                    | ExprKind::ClassExpr { .. }
                    | ExprKind::Match { .. }
                    | ExprKind::Is { .. }
                    | ExprKind::With { .. }
                    | ExprKind::MetaAccess { .. } => None,
                },
                ExprKind::IntLiteral { .. }
                | ExprKind::FloatLiteral { .. }
                | ExprKind::BigIntLiteral { .. }
                | ExprKind::DecimalLiteral { .. }
                | ExprKind::StrLiteral { .. }
                | ExprKind::CharLiteral { .. }
                | ExprKind::BoolLiteral { .. }
                | ExprKind::NullLiteral
                | ExprKind::RegexLiteral { .. }
                | ExprKind::Template { .. }
                | ExprKind::TaggedTemplate { .. }
                | ExprKind::Missing
                | ExprKind::This
                | ExprKind::Super
                | ExprKind::Array { .. }
                | ExprKind::Object { .. }
                | ExprKind::Tuple { .. }
                | ExprKind::Record { .. }
                | ExprKind::Unary { .. }
                | ExprKind::Update { .. }
                | ExprKind::Binary { .. }
                | ExprKind::Logical { .. }
                | ExprKind::Assign { .. }
                | ExprKind::Conditional { .. }
                | ExprKind::Member { .. }
                | ExprKind::Call { .. }
                | ExprKind::New { .. }
                | ExprKind::Function { .. }
                | ExprKind::Arrow { .. }
                | ExprKind::Sequence { .. }
                | ExprKind::Paren { .. }
                | ExprKind::Await { .. }
                | ExprKind::Spawn { .. }
                | ExprKind::Yield { .. }
                | ExprKind::Spread { .. }
                | ExprKind::Pipeline { .. }
                | ExprKind::Range { .. }
                | ExprKind::NonNull { .. }
                | ExprKind::Try { .. }
                | ExprKind::As { .. }
                | ExprKind::Satisfies { .. }
                | ExprKind::ClassExpr { .. }
                | ExprKind::Match { .. }
                | ExprKind::Is { .. }
                | ExprKind::With { .. }
                | ExprKind::MetaAccess { .. } => None,
            };

            if let Some(callee_name) = callee_name {
                let callee_name_str = interner.resolve(callee_name);
                if let Some(ty) = fn_map.get(callee_name_str) {
                    if let Some(tps) = fn_type_params.get(callee_name_str) {
                        let mut mapping: FxHashMap<varn_core::Atom, Type> = FxHashMap::default();
                        for (i, tp) in tps.iter().enumerate() {
                            if let Some(node) = type_args.get(i) {
                                let resolved = resolve_type_node(node, ctx, table);
                                mapping.insert(varn_core::Atom::of(tp), resolved);
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
            match table.get(callee_ty.0) {
                TypeKind::Fn(fid) => Some(Type::resolved(table.get_function(fid).return_type)),
                TypeKind::Primitive(_)
                | TypeKind::Builtin(_)
                | TypeKind::Literal(_)
                | TypeKind::This
                | TypeKind::Array(_)
                | TypeKind::Union(_)
                | TypeKind::Intersection(_)
                | TypeKind::Tuple(_)
                | TypeKind::Named(..)
                | TypeKind::Generic(..)
                | TypeKind::TemplateLiteral(_)
                | TypeKind::Object(_)
                | TypeKind::Typeof(_)
                | TypeKind::KeyOf(_)
                | TypeKind::IndexedAccess { .. }
                | TypeKind::Mapped { .. }
                | TypeKind::Conditional { .. }
                | TypeKind::Infer(_)
                | TypeKind::EnumVariant { .. }
                | TypeKind::TypePredicate { .. } => None,
            }
        }

        ExprKind::New {
            callee, type_args, ..
        } => {
            if let ExprKind::Identifier { name } = &ast_arena.expr(*callee).kind {
                let name_str = interner.resolve(*name);
                if !type_args.is_empty() {
                    let mut args = Vec::new();
                    for node in type_args {
                        args.push(resolve_type_node(node, ctx, table));
                    }
                    return Some(Type::generic(Arc::from(name_str), args, table));
                }
                if name_str == varn_core::BuiltinType::Map.name() {
                    return Some(Type::generic(
                        Arc::from(name_str),
                        vec![Type::Dynamic, Type::Dynamic],
                        table,
                    ));
                }
                return Some(Type::named(Arc::from(name_str), table));
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
            match table.get(ty.0) {
                TypeKind::Generic(name, args, _)
                    if (interner.get(varn_core::BuiltinType::Task.name()) == Some(name)
                        || interner.get(varn_core::BuiltinType::TaskHandle.name())
                            == Some(name))
                        && table.get_list(args).len() == 1 =>
                {
                    Some(Type::resolved(table.get_list(args)[0]))
                }
                TypeKind::Primitive(_)
                | TypeKind::Builtin(_)
                | TypeKind::Literal(_)
                | TypeKind::This
                | TypeKind::Array(_)
                | TypeKind::Union(_)
                | TypeKind::Intersection(_)
                | TypeKind::Tuple(_)
                | TypeKind::Named(..)
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
                | TypeKind::EnumVariant { .. }
                | TypeKind::TypePredicate { .. } => Some(ty),
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

        ExprKind::BigIntLiteral { .. }
        | ExprKind::DecimalLiteral { .. }
        | ExprKind::CharLiteral { .. }
        | ExprKind::NullLiteral
        | ExprKind::RegexLiteral { .. }
        | ExprKind::Template { .. }
        | ExprKind::TaggedTemplate { .. }
        | ExprKind::Missing
        | ExprKind::Super
        | ExprKind::Array { .. }
        | ExprKind::Object { .. }
        | ExprKind::Tuple { .. }
        | ExprKind::Record { .. }
        | ExprKind::Unary { .. }
        | ExprKind::Update { .. }
        | ExprKind::Logical { .. }
        | ExprKind::Assign { .. }
        | ExprKind::Member { .. }
        | ExprKind::Sequence { .. }
        | ExprKind::Spawn { .. }
        | ExprKind::Yield { .. }
        | ExprKind::Spread { .. }
        | ExprKind::Range { .. }
        | ExprKind::NonNull { .. }
        | ExprKind::Try { .. }
        | ExprKind::Satisfies { .. }
        | ExprKind::ClassExpr { .. }
        | ExprKind::Is { .. }
        | ExprKind::With { .. }
        | ExprKind::MetaAccess { .. } => Some(Type::Dynamic),
    }
}
