mod assign;
mod binary_ops;
mod calls;
mod closures;
mod const_int;
mod contextual;
mod control;
mod exhaustiveness;
mod match_arm;
pub(crate) mod members;
mod misc;
mod operator_capability;
mod ops;

use crate::checker::Checker;
use varn_core::ast::{ExprId, ExprKind, TemplatePart};
use varn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(crate) fn check_expr(&mut self, expr: ExprId, bind: &BindResult) {
        let arena = self.ast_arena;
        self.check_expr_no_record(expr, bind);
        let range = arena.expr(expr).range;
        let start = range.start.offset;
        let end = range.end.offset.saturating_sub(1);

        if self.record_expr_types {
            self.node_scopes.insert(start, self.current_scope);
        }
        let ty = self.infer_type(expr, bind);

        let symbol_id = match (&arena.expr(expr).kind, self.record_expr_types) {
            (ExprKind::Identifier { name }, true) => {
                let scope = bind.scopes.get(self.current_scope);
                scope.resolve(*name, &bind.scopes)
            }
            _ => None,
        };

        let refined = self.refine(expr, bind);
        debug_assert!(
            refined.as_ref().is_none_or(|r| !r.is_dynamic()),
            "a refinement must tell codegen MORE than the checked type, and              `dynamic` is the absence of information"
        );

        let seq = self.expr_seq;
        self.expr_seq += 1;

        self.expr_table.insert(
            expr.index(),
            varn_sem::output::TypeEntry {
                ty,
                refined,
                symbol_id,
                start,
                end,
                seq,
            },
        );
    }

    fn check_expr_no_record(&mut self, expr: ExprId, bind: &BindResult) {
        let arena = self.ast_arena;
        let range = arena.expr(expr).range;
        match &arena.expr(expr).kind {
            ExprKind::Missing => {}
            ExprKind::Arrow {
                params,
                return_type,
                body,
                is_async,
                ..
            } => self.check_arrow(params, return_type, body, *is_async, range, bind),
            ExprKind::Function {
                return_type,
                body,
                is_async,
                ..
            } => self.check_function_expr(return_type, *body, *is_async, range, bind),
            ExprKind::As { expression, .. } => self.check_expr(*expression, bind),
            ExprKind::Is { expression, .. } => self.check_expr(*expression, bind),
            ExprKind::Satisfies {
                expression,
                type_ann,
            } => self.check_satisfies(*expression, type_ann, range, bind),
            ExprKind::Await { argument } => self.check_await(*argument, range, bind),
            ExprKind::Spawn { argument } => {
                if self.pure_scope.is_some() {
                    self.forbid_pure("spawn tasks (pure functions are synchronous)", range);
                }
                self.check_expr(*argument, bind)
            }
            ExprKind::Try { expression } => self.check_try(*expression, range, bind),
            ExprKind::Yield { argument, .. } => self.check_yield(*argument, range, bind),
            ExprKind::Unary { op, operand, .. } => {
                self.check_unary(expr, *op, *operand, arena, bind)
            }
            ExprKind::Binary { left, right, op } => {
                self.check_binary(expr, *left, *right, *op, range, arena, bind)
            }
            ExprKind::Logical { left, right, .. } => {
                self.check_expr(*left, bind);
                self.check_expr(*right, bind);
            }
            ExprKind::Assign { target, value, .. } => {
                self.check_assign(*target, *value, range, bind)
            }
            ExprKind::Call {
                callee,
                args,
                type_args,
                ..
            } => {
                let (callee, args, type_args) = (*callee, args.clone(), type_args.clone());
                self.check_call_expr(callee, &args, &type_args, &range, expr.index(), bind)
            }
            ExprKind::New { callee, args, .. } => self.check_new(*callee, args, range, arena, bind),

            ExprKind::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.check_expr(*test, bind);
                self.check_expr(*consequent, bind);
                self.check_expr(*alternate, bind);
            }

            ExprKind::Member {
                object,
                property,
                computed,
                optional,
            } => {
                let (object, property, computed, optional) =
                    (*object, *property, *computed, *optional);
                self.check_member_expr(expr, object, property, computed, optional, &range, bind)
            }

            ExprKind::Paren { expression } => self.check_expr(*expression, bind),
            ExprKind::NonNull { expression } => self.check_expr(*expression, bind),

            ExprKind::Array { elements } => {
                let elements = elements.clone();
                self.check_array_with_context(&elements, bind)
            }

            ExprKind::Tuple { elements } => {
                for e in elements.clone() {
                    self.check_expr(e, bind);
                }
            }

            ExprKind::Object { properties } => {
                let properties = properties.clone();
                self.check_object_with_context(&properties, bind)
            }
            ExprKind::Record { properties } => {
                let properties = properties.clone();
                self.check_object_with_context(&properties, bind)
            }

            ExprKind::Template { parts } => {
                for p in parts.clone() {
                    if let TemplatePart::Interpolation(e) = p {
                        self.check_expr(e, bind);
                    }
                }
            }

            ExprKind::Sequence { expressions } => {
                for e in expressions.clone() {
                    self.check_expr(e, bind);
                }
            }

            ExprKind::ClassExpr { declaration } => {
                let declaration = declaration.clone();
                self.check_decl(&varn_core::ast::Decl::Class((*declaration).clone()), bind);
            }

            ExprKind::Match { subject, cases } => {
                self.check_match(*subject, cases, expr, range, bind)
            }

            ExprKind::Update { operand, .. } => self.check_update(*operand, bind),
            ExprKind::Spread { argument } => self.check_expr(*argument, bind),

            ExprKind::Pipeline { left, right } => self.check_pipeline(*left, *right, bind),

            ExprKind::Range { start, end, .. } => self.check_range(*start, *end, range, bind),

            ExprKind::TaggedTemplate { tag, template, .. } => {
                self.check_tagged_template(*tag, *template, range, bind)
            }

            ExprKind::With { object, properties } => {
                self.check_with_object(*object, properties, range, bind)
            }

            ExprKind::MetaAccess { target, .. } => {
                self.check_meta_access(*target, expr, range, bind)
            }

            ExprKind::Identifier { name } => self.check_identifier(*name, range, bind),

            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::RegexLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::Super
            | ExprKind::This => {}
        }
    }
}
