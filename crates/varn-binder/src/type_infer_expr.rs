use super::inference_utils::{build_fn_type, widen_literal};
use super::type_infer_member::{ctx_resolve_text, infer_member};
use super::type_infer_ops::{infer_binary, infer_new, infer_object};
use varn_core::ast::operators::{LogicalOp, UnaryOp};
use varn_core::ast::{ArrayEl, ArrowBody, AstArena, ExprId, ExprKind, MatchBody};
use varn_core::TypeKind;
use varn_sem::types::{CheckerTyTable, Type};

pub fn infer_expr_type(
    id: ExprId,
    arena: &AstArena,
    ctx: Option<&dyn varn_sem::types::TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    match &arena.expr(id).kind {
        ExprKind::IntLiteral { .. } => Type::Int,
        ExprKind::FloatLiteral { .. } => Type::Float,
        ExprKind::DecimalLiteral { .. } => Type::Decimal,
        ExprKind::BigIntLiteral { .. } => Type::BigInt,
        ExprKind::StrLiteral { .. } => Type::Str,
        ExprKind::CharLiteral { .. } => Type::Char,
        ExprKind::BoolLiteral { .. } => Type::Bool,
        ExprKind::NullLiteral => Type::Null,
        ExprKind::Template { .. } => Type::Str,
        ExprKind::Paren { expression } => infer_expr_type(*expression, arena, ctx, table),
        ExprKind::As { type_ann, .. } => {
            super::type_resolution::resolve_type_node(type_ann, ctx, table)
        }
        ExprKind::Is { .. } => Type::Bool,
        ExprKind::Satisfies { expression, .. } => infer_expr_type(*expression, arena, ctx, table),
        ExprKind::Await { argument } => {
            let inner = infer_expr_type(*argument, arena, ctx, table);
            match table.get(inner.0) {
                TypeKind::Generic(name, args, _)
                    if ctx.and_then(|c| c.interner()).is_some_and(|i| {
                        i.get(varn_core::BuiltinType::Task.name()) == Some(name)
                    }) && table.get_list(args).len() == 1 =>
                {
                    Type::resolved(table.get_list(args)[0])
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
                | TypeKind::TypePredicate { .. } => inner,
            }
        }
        ExprKind::NonNull { expression } => infer_expr_type(*expression, arena, ctx, table),
        ExprKind::Try { expression } => {
            let inner = infer_expr_type(*expression, arena, ctx, table);
            let ok_first = inner
                .core_sum(table, |a| ctx_resolve_text(ctx, a))
                .and_then(|(_, args)| args.first().copied());
            if let Some(ok) = ok_first {
                ok
            } else if inner.is_nullable(table) {
                inner.non_nullified(table)
            } else {
                inner
            }
        }
        ExprKind::Logical {
            op, left, right, ..
        } => match op {
            LogicalOp::Nullish => {
                let rhs = infer_expr_type(*right, arena, ctx, table);
                if !rhs.is_dynamic() {
                    return rhs;
                }
                infer_expr_type(*left, arena, ctx, table)
            }
            LogicalOp::And | LogicalOp::Or => {
                let l = infer_expr_type(*left, arena, ctx, table);
                let r = infer_expr_type(*right, arena, ctx, table);
                if l.0 == r.0 {
                    l
                } else {
                    Type::Dynamic
                }
            }
        },
        ExprKind::Member {
            object,
            property,
            computed,
            ..
        } => infer_member(*object, *property, *computed, arena, ctx, table),
        ExprKind::Unary { op, operand, .. } => {
            let inner = infer_expr_type(*operand, arena, ctx, table);
            match op {
                UnaryOp::Minus | UnaryOp::Plus => inner,
                UnaryOp::Not => Type::Bool,
                UnaryOp::BitNot => match table.get(inner.0) {
                    TypeKind::Primitive(varn_core::LangPrimitive::Int) => Type::Int,
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
                    | TypeKind::TypePredicate { .. } => Type::Dynamic,
                },
                _ => Type::Dynamic,
            }
        }
        ExprKind::Binary {
            op, left, right, ..
        } => infer_binary(op, *left, *right, arena, ctx, table),
        ExprKind::Tuple { elements } => {
            let ids: Vec<_> = elements
                .iter()
                .map(|e| infer_expr_type(*e, arena, ctx, table).0)
                .collect();
            let list = table.intern_list(&ids);
            Type::resolved(table.intern(TypeKind::Tuple(list)))
        }
        ExprKind::Array { elements } => {
            for el in elements {
                if let ArrayEl::Expr(first) = el {
                    let elem_ty = infer_expr_type(*first, arena, ctx, table);
                    if !elem_ty.is_dynamic() {
                        let widened = widen_literal(elem_ty);
                        return Type::array(widened, table);
                    }
                }
            }
            Type::Dynamic
        }
        ExprKind::Call { callee, .. } => {
            let callee_ty = infer_expr_type(*callee, arena, ctx, table);
            if let TypeKind::Fn(fid) = table.get(callee_ty.0) {
                return Type::resolved(table.get_function(fid).return_type);
            }
            Type::Dynamic
        }
        ExprKind::New {
            callee, type_args, ..
        } => infer_new(*callee, type_args, arena, ctx, table),
        ExprKind::Identifier { name } => ctx
            .and_then(|c| c.interner().map(|i| (c, i)))
            .and_then(|(c, i)| c.resolve_symbol(i.resolve(*name)))
            .unwrap_or(Type::Dynamic),
        ExprKind::This => ctx
            .and_then(|c| c.resolve_symbol("this"))
            .unwrap_or(Type::This),
        ExprKind::Arrow {
            params,
            return_type,
            body,
            ..
        } => {
            let mut inferred_ret = Type::Dynamic;
            if let ArrowBody::Expr(e) = body.as_ref() {
                inferred_ret = infer_expr_type(*e, arena, ctx, table);
            }
            build_fn_type(params, return_type, true, ctx, table, inferred_ret)
        }
        ExprKind::Function {
            params,
            return_type,
            is_generator,
            ..
        } => {
            let ret = if *is_generator {
                varn_sem::types::generator_of(Type::Dynamic, false, table)
            } else {
                Type::Dynamic
            };
            build_fn_type(params, return_type, false, ctx, table, ret)
        }
        ExprKind::Match { cases, .. } => {
            let mut expr_arm_ty = None;
            for case in cases {
                match &case.body {
                    MatchBody::Expr(e) => {
                        let ty = infer_expr_type(*e, arena, ctx, table);
                        if !ty.is_dynamic() && !ty.is_never() {
                            expr_arm_ty = Some(ty);
                            break;
                        }
                    }
                    MatchBody::Block(_) => {}
                }
            }
            expr_arm_ty.unwrap_or(Type::Dynamic)
        }
        ExprKind::Object { properties } => infer_object(properties, arena, ctx, table),
        ExprKind::Range { start, .. } => {
            let bound = infer_expr_type(*start, arena, ctx, table);
            Type::range_over(&bound, table)
        }
        ExprKind::Pipeline { right, .. } => infer_expr_type(*right, arena, ctx, table),
        ExprKind::RegexLiteral { .. }
        | ExprKind::TaggedTemplate { .. }
        | ExprKind::Missing
        | ExprKind::Super
        | ExprKind::Record { .. }
        | ExprKind::Update { .. }
        | ExprKind::Assign { .. }
        | ExprKind::Conditional { .. }
        | ExprKind::Sequence { .. }
        | ExprKind::Spawn { .. }
        | ExprKind::Yield { .. }
        | ExprKind::Spread { .. }
        | ExprKind::ClassExpr { .. }
        | ExprKind::With { .. }
        | ExprKind::MetaAccess { .. } => Type::Dynamic,
    }
}
