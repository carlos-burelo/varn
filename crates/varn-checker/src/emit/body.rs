use std::sync::Arc;
use varn_core::ast::{ExprId, ExprKind};
use varn_tir::{BackendTy, DynReason, Resolution, TirExpr, TirExprKind};

mod context;
mod expr_assign;
mod expr_async;
mod expr_binary;
mod expr_call;
mod expr_closure;
mod expr_collection;
mod expr_convert;
mod expr_member;
mod expr_misc;
mod expr_new;
mod expr_special;
mod expr_template;
mod expr_try;
mod expr_update;
mod finally_splice;
mod match_pattern;
mod scope;
mod small_utils;
mod stmt_block;
mod stmt_decl;
mod stmt_for;
mod stmt_for_of;
mod stmt_match;
mod stmt_switch;
mod stmt_try;

use expr_closure::ClosureBody;

pub(super) use context::{FnEmitter, ModuleCtx};

use small_utils::{assign_bin_op, span_of};

impl<'a> FnEmitter<'a> {
    fn lower_expr_node(&mut self, e: ExprId) -> TirExpr {
        let ty = self.expr_ty(e);
        let span = span_of(self.ast_arena, e);

        let kind = match &self.ast_arena.expr(e).kind {
            ExprKind::IntLiteral { value, .. } => Some(TirExprKind::IntLit(*value)),
            ExprKind::FloatLiteral { value, .. } => Some(TirExprKind::FloatLit(*value)),
            ExprKind::BoolLiteral { value } => Some(TirExprKind::BoolLit(*value)),
            ExprKind::StrLiteral { value } => Some(TirExprKind::StrLit(Arc::from(value.as_str()))),
            ExprKind::CharLiteral { value } => Some(TirExprKind::CharLit(*value)),
            ExprKind::NullLiteral => Some(TirExprKind::NullLit),

            ExprKind::Identifier { name } => {
                let name_str = self.m.interner.resolve(*name);
                return TirExpr {
                    kind: TirExprKind::Var,
                    ty,
                    res: self.resolve_name(name_str),
                    span,
                };
            }

            ExprKind::Paren { expression } => return self.lower_expr(*expression),

            ExprKind::Binary { op, left, right } => {
                if self.m.desugar.operator_calls.contains(&e.index()) {
                    if let Some(method) = varn_core::capability::binary_operator_method(*op) {
                        return self.lower_operator_call(method, *left, Some(*right), ty, span);
                    }
                }
                return self.lower_binary(*op, *left, *right, ty, span);
            }
            ExprKind::Unary {
                op,
                operand,
                prefix: _,
            } => {
                if self.m.desugar.operator_calls.contains(&e.index()) {
                    if let Some(method) = varn_core::capability::unary_operator_method(*op) {
                        return self.lower_operator_call(method, *operand, None, ty, span);
                    }
                }
                return self.lower_unary(*op, *operand, ty, span);
            }

            ExprKind::This => {
                let this_ty = self
                    .this_enum
                    .map(BackendTy::Enum)
                    .or_else(|| self.this_class.map(BackendTy::Class))
                    .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated));
                return TirExpr {
                    kind: TirExprKind::Var,
                    ty: this_ty,
                    res: Resolution::None,
                    span,
                };
            }

            ExprKind::Member {
                object,
                property,
                computed,
                optional,
            } => return self.lower_member(*object, *property, *computed, *optional, ty, span),

            ExprKind::Logical { op, left, right } => {
                return self.lower_logical(*op, *left, *right, ty, span)
            }

            ExprKind::Template { parts } => return self.lower_template(parts, span),

            ExprKind::Match { subject, cases } => {
                let subject = *subject;
                return self.lower_match_expr(subject, cases, ty, span);
            }

            ExprKind::Function {
                params,
                body,
                is_async,
                is_generator,
                ..
            } => {
                return self.lower_closure(
                    params,
                    ClosureBody::Stmt(*body),
                    *is_async,
                    *is_generator,
                    ty,
                    span,
                )
            }
            ExprKind::Arrow {
                params,
                body,
                is_async,
                ..
            } => {
                let cb = match body.as_ref() {
                    varn_core::ast::ArrowBody::Expr(e) => ClosureBody::Expr(*e),
                    varn_core::ast::ArrowBody::Block(s) => ClosureBody::Stmt(*s),
                };
                return self.lower_closure(params, cb, *is_async, false, ty, span);
            }

            ExprKind::Await { argument } => {
                return self.lower_await(*argument, ty, span);
            }
            ExprKind::Yield { argument, delegate } => {
                return self.lower_yield(*argument, *delegate, span);
            }

            ExprKind::Call {
                callee,
                args,
                optional: _,
                type_args: _,
            } => {
                let callee = *callee;
                return self.lower_call(e.index(), callee, args, ty, span);
            }

            ExprKind::Array { elements } => {
                return self.lower_array(elements, ty, span);
            }
            ExprKind::Tuple { elements } => {
                return self.lower_tuple(elements, ty, span);
            }
            ExprKind::Record { properties } => {
                return self.lower_record(properties, ty, span);
            }
            ExprKind::Object { properties } => {
                return self.lower_object(properties, ty, span);
            }
            ExprKind::New { callee, args, .. } => {
                let callee = *callee;
                return self.lower_new(e.index(), callee, args, ty, span);
            }

            ExprKind::NonNull { expression } => {
                let inner = self.lower_expr(*expression);
                let nn = inner.ty.non_nullable(self.tt);
                if inner.ty == nn {
                    return inner;
                }
                return TirExpr {
                    kind: TirExprKind::Cast {
                        operand: Box::new(inner),
                    },
                    ty: nn,
                    res: Resolution::None,
                    span,
                };
            }

            ExprKind::As { expression, .. } => {
                return self.lower_as_cast(*expression, ty, span);
            }
            ExprKind::Satisfies { expression, .. } => return self.lower_expr(*expression),

            ExprKind::Sequence { expressions } => {
                return self.lower_sequence(expressions, span);
            }

            ExprKind::Pipeline { left, right } => {
                return self.lower_pipeline(*left, *right, ty, span);
            }

            ExprKind::With { object, properties } => {
                return self.lower_with(*object, properties, ty, span);
            }

            ExprKind::Assign { op, target, value }
                if matches!(assign_bin_op(*op), Ok(None))
                    && matches!(&self.ast_arena.expr(*target).kind, ExprKind::Member { .. })
                    && self
                        .m
                        .desugar
                        .extension_set_members
                        .contains_key(&self.ast_arena.expr(*target).range.start.offset) =>
            {
                let (target, value) = (*target, *value);
                return self.lower_extension_assign(target, value, ty, span);
            }
            ExprKind::Assign { op, target, value }
                if matches!(
                    &self.ast_arena.expr(*target).kind,
                    ExprKind::Member {
                        computed: true,
                        optional: false,
                        ..
                    }
                ) =>
            {
                let (op, target, value) = (*op, *target, *value);
                let ExprKind::Member {
                    object, property, ..
                } = &self.ast_arena.expr(target).kind
                else {
                    unreachable!()
                };
                let (object, property) = (*object, *property);
                return self.lower_index_assign(op, object, property, value, ty, span);
            }
            ExprKind::Assign { op, target, value }
                if matches!(
                    self.ast_arena.expr(*target).kind,
                    ExprKind::Identifier { .. } | ExprKind::Member { .. }
                ) =>
            {
                let (op, target, value) = (*op, *target, *value);
                return self.lower_plain_assign(op, target, value, ty, span);
            }

            ExprKind::Update {
                op,
                operand,
                prefix,
            } if matches!(
                &self.ast_arena.expr(*operand).kind,
                ExprKind::Member {
                    computed: true,
                    optional: false,
                    ..
                }
            ) =>
            {
                let (op, operand, prefix) = (*op, *operand, *prefix);
                let ExprKind::Member {
                    object, property, ..
                } = &self.ast_arena.expr(operand).kind
                else {
                    unreachable!()
                };
                let (object, property) = (*object, *property);
                return self.lower_index_update(op, object, property, prefix, ty, span);
            }
            ExprKind::Update {
                op,
                operand,
                prefix,
            } if matches!(
                self.ast_arena.expr(*operand).kind,
                ExprKind::Identifier { .. } | ExprKind::Member { .. }
            ) =>
            {
                let (op, operand, prefix) = (*op, *operand, *prefix);
                return self.lower_plain_update(op, operand, prefix, span);
            }

            ExprKind::DecimalLiteral { raw } => self.lower_decimal_kind(*raw),
            ExprKind::BigIntLiteral { raw } => self.lower_bigint_kind(*raw),
            ExprKind::RegexLiteral { pattern, flags } => {
                let s = TirExpr {
                    kind: TirExprKind::StrLit(Arc::from(format!("/{pattern}/{flags}"))),
                    ty: BackendTy::Str,
                    res: Resolution::None,
                    span,
                };
                return self.cast_to(s, ty);
            }

            ExprKind::Spawn { argument } => {
                let inner = self.lower_expr(*argument);
                return self.cast_to(inner, ty);
            }
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => {
                return self.lower_range(*start, *end, *inclusive, ty, span);
            }

            ExprKind::Is {
                expression,
                type_ann,
            } => {
                return self.lower_is(*expression, type_ann, span);
            }

            ExprKind::MetaAccess { target, property } => {
                let obj = self.lower_expr(*target);
                let key: Arc<str> = Arc::from(format!("::{}", self.m.interner.resolve(*property)));
                return TirExpr {
                    kind: TirExprKind::Field {
                        object: Box::new(obj),
                        name: key.clone(),
                    },
                    ty,
                    res: Resolution::ByName {
                        name: key,
                        why: DynReason::Unannotated,
                    },
                    span,
                };
            }

            ExprKind::Super => {
                let sty = self
                    .this_class
                    .map(BackendTy::Class)
                    .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated));
                return TirExpr {
                    kind: TirExprKind::Var,
                    ty: sty,
                    res: Resolution::None,
                    span,
                };
            }
            ExprKind::TaggedTemplate { tag, template } => {
                return self.lower_tagged_template(*tag, *template, ty, span);
            }

            ExprKind::Conditional {
                test,
                consequent,
                alternate,
            } => {
                let (test, consequent, alternate) = (*test, *consequent, *alternate);
                return self.lower_conditional(test, consequent, alternate, ty, span);
            }

            ExprKind::ClassExpr { .. } => {
                return TirExpr {
                    kind: TirExprKind::Var,
                    ty,
                    res: self.resolve_name("<anon>"),
                    span,
                };
            }

            ExprKind::Try { expression } => {
                return self.lower_try_expr(*expression, ty, span);
            }

            _ => None,
        };

        match kind {
            Some(kind) => TirExpr {
                kind,
                ty,
                res: Resolution::None,
                span,
            },
            None => TirExpr {
                kind: TirExprKind::NullLit,
                ty: BackendTy::Dynamic(DynReason::Unannotated),
                res: Resolution::None,
                span,
            },
        }
    }
}
