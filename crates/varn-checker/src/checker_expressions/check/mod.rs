mod calls;
mod contextual;
mod exhaustiveness;
pub(crate) mod members;

use super::helpers::{base_type, closest_in_list, op_str};
use super::infer::member_binary::normalize_for_binary;
use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::Type;
use std::rc::Rc;
use varn_core::ast::operators::BinaryOp;
use varn_core::ast::{ArrowBody, Expr, ExprKind, MatchBody, MatchPattern, TemplatePart};
use varn_core::{Diagnostic, ErrorCode, IntrinsicType, Suggestion, TypeKind, TypeTag};

impl<'r> Checker<'r> {
    /// Emit the literal-overflow diagnostic for `expr`.
    ///
    /// Callers gate on the operands NOT overflowing themselves, so a nested
    /// overflow is named once at its innermost expression instead of again at
    /// every enclosing one.
    fn report_int_overflow(&mut self, expr: &Expr) {
        self.diagnostics.push(
            Diagnostic::error(
                ErrorCode::IntegerOverflow,
                format!(
                    "this expression overflows int ({}..={})",
                    varn_core::INT_MIN,
                    varn_core::INT_MAX
                ),
            )
            .with_file(self.source_file.clone())
            .with_range(expr.range),
        );
    }

    pub(crate) fn check_expr(&mut self, expr: &Expr, bind: &BindResult) {
        self.check_expr_no_record(expr, bind);
        let start = expr.range.start.offset;
        let end = expr.range.end.offset.saturating_sub(1);
        // By BYTE OFFSET, like the 18 `record_scope` sites, and only when a
        // caller asked for the table. This line used to key by `expr.id` —
        // mixing AST ids into a map whose only consumers (`scope_at_offset`
        // and the LSP's `PositionalIndex`) read every key as an offset, so the
        // ids landed in the positional index as positions that do not exist.
        // It also ran unconditionally, building that map on the compile path
        // for nobody to read.
        if self.record_expr_types {
            self.node_scopes.insert(start, self.current_scope);
        }
        let ty = self.infer_type(expr, bind);

        // Resolving the identifier's symbol costs a scope walk and only tooling
        // reads it, so it is not paid for on a compile.
        let symbol_id = match (&expr.kind, self.record_expr_types) {
            (ExprKind::Identifier { name }, true) => {
                let scope = bind.scopes.get(self.current_scope);
                scope.resolve(name.as_ref(), &bind.scopes)
            }
            _ => None,
        };

        // ONE write, into the one table. The positional map the editor reads is
        // projected from this after the check (`project_positional_types`);
        // writing both here is how they came to disagree.
        // The refinement lane: what a prover established beyond what the
        // checker committed to. Computed here, once, so nothing downstream
        // has to re-derive it from a different engine. See `checker::refine`.
        let refined = self.refine(expr, bind);
        debug_assert!(
            refined.as_ref().is_none_or(|r| !r.is_dynamic()),
            "a refinement must tell codegen MORE than the checked type, and              `dynamic` is the absence of information"
        );

        let seq = self.expr_seq;
        self.expr_seq += 1;
        self.expr_table.insert(
            expr.id,
            crate::checker::TypeEntry {
                ty,
                refined,
                symbol_id,
                start,
                end,
                seq,
            },
        );
    }

    fn check_expr_no_record(&mut self, expr: &Expr, bind: &BindResult) {
        match &expr.kind {
            // Nothing to check: the parser already reported the syntax error.
            // Emitting a second diagnostic here would paint the file red for
            // code the user is still in the middle of typing.
            ExprKind::Missing => {}
            ExprKind::Arrow {
                params,
                return_type,
                body,
                ..
            } => {
                let saved_expected = self.expected_return_type.take();

                self.expected_return_type = return_type
                    .as_ref()
                    .map(|rt| self.resolve_type_node_cached(rt, bind))
                    .or_else(|| self.expected_return_from_fn_type());

                let saved_scope = self.current_scope;
                if let Some(fn_scope) = self.next_child_scope(bind) {
                    self.current_scope = fn_scope;
                    self.record_scope_span(
                        expr.range.start.offset,
                        expr.range.end.offset,
                        fn_scope,
                    );
                }

                let mut injected_type_params: Vec<Rc<str>> = vec![];
                if let Some(expected_fn) = self.expected_fn_type() {
                    for ep in &expected_fn.params {
                        if let varn_core::TypeKind::Named(n, _) = &ep.ty.0 {
                            if !varn_core::IntrinsicType::from_str(n.as_ref()).is_some() {
                                injected_type_params.push(n.clone());
                            }
                        }
                    }
                    if let varn_core::TypeKind::Named(n, _) = &expected_fn.return_type.0 {
                        if !varn_core::IntrinsicType::from_str(n.as_ref()).is_some() {
                            injected_type_params.push(n.clone());
                        }
                    }
                    for tp in &injected_type_params {
                        self.active_type_params.insert(tp.clone());
                    }
                    self.apply_contextual_arrow_params(params, &expected_fn, bind);
                }

                let saved_in_function = self.in_function;
                let saved_loop_depth = self.loop_depth;
                let saved_switch_depth = self.switch_depth;
                self.in_function = true;
                self.loop_depth = 0;
                self.switch_depth = 0;

                match body.as_ref() {
                    ArrowBody::Block(stmt) => self.check_stmt(stmt, bind),
                    ArrowBody::Expr(e) => {
                        self.check_expr(e, bind);
                        let actual = self.infer_type(e, bind);
                        if let Some(expected) = self.expected_return_type.clone() {
                            let is_tp = matches!(&expected.0, varn_core::TypeKind::Named(n, _) if self.active_type_params.contains(n.as_ref()));
                            let is_void = matches!(
                                expected.0,
                                varn_core::TypeKind::Intrinsic(varn_core::TypeTag::Void)
                            );
                            if !is_tp
                                && !is_void
                                && !self.types_compatible_cached(&expected, &actual, Some(bind))
                            {
                                self.emit(
                                    Diagnostic::error(ErrorCode::TypeMismatch, format!(
                                        "type mismatch: arrow function is declared to return '{expected}', but returns '{actual}'"
                                    ))
                                    .with_range(expr.range),
                                );
                            }
                        }
                    }
                }

                self.in_function = saved_in_function;
                self.loop_depth = saved_loop_depth;
                self.switch_depth = saved_switch_depth;

                self.current_scope = saved_scope;
                self.expected_return_type = saved_expected;
                for tp in &injected_type_params {
                    self.active_type_params.remove(tp.as_ref());
                }
            }
            ExprKind::Function {
                return_type, body, ..
            } => {
                let saved_expected = self.expected_return_type.take();
                self.expected_return_type = return_type
                    .as_ref()
                    .map(|rt| self.resolve_type_node_cached(rt, bind));

                let saved_scope = self.current_scope;
                if let Some(fn_scope) = self.next_child_scope(bind) {
                    self.current_scope = fn_scope;
                    self.record_scope_span(
                        expr.range.start.offset,
                        expr.range.end.offset,
                        fn_scope,
                    );
                }

                self.in_function_body(|c| c.check_stmt(body, bind));

                self.current_scope = saved_scope;
                self.expected_return_type = saved_expected;
            }
            ExprKind::As { expression, .. } => self.check_expr(expression, bind),
            ExprKind::Is { expression, .. } => self.check_expr(expression, bind),
            ExprKind::Satisfies {
                expression,
                type_ann,
            } => {
                self.check_expr(expression, bind);
                let declared_ty = self.resolve_type_node_cached(type_ann, bind);
                let inferred_ty = self.infer_type(expression, bind);
                if !self.types_compatible_cached(&declared_ty, &inferred_ty, Some(bind)) {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::InvalidSatisfies,
                            format!(
                                "expression does not satisfy '{declared_ty}': got '{inferred_ty}'"
                            ),
                        )
                        .with_range(expr.range),
                    );
                }
            }
            ExprKind::Await { argument } => {
                self.check_expr(argument, bind);
                let arg_ty = self.infer_type(argument, bind);
                if !arg_ty.is_dynamic() && !crate::types::is_awaitable(&arg_ty) {
                    self.emit(
                        Diagnostic::warning(
                            ErrorCode::TypeMismatch,
                            format!("'await' applied to non-Future type '{arg_ty}' has no effect"),
                        )
                        .with_range(expr.range),
                    );
                }
            }
            ExprKind::Spawn { argument } => self.check_expr(argument, bind),
            ExprKind::Try { expression } => self.check_expr(expression, bind),
            ExprKind::Yield { argument, .. } => {
                let ty = if let Some(arg) = argument {
                    self.check_expr(arg, bind);
                    self.infer_type(arg, bind)
                } else {
                    Type::Void
                };
                if let Some(yields) = &mut self.yielded_types {
                    yields.push(ty);
                }
            }
            ExprKind::Unary { operand, .. } => {
                self.check_expr(operand, bind);
                if overflows_int_literal(expr) && !overflows_int_literal(operand) {
                    self.report_int_overflow(expr);
                }
            }
            ExprKind::Binary { left, right, op } => {
                self.check_expr(left, bind);
                self.check_expr(right, bind);

                // An all-literal arithmetic expression whose value leaves the
                // `int` range is reported HERE, where the span is, rather than
                // waiting for the run-time raise. Reported only when neither
                // operand already overflows on its own, so a nested overflow
                // names its innermost expression once.
                if overflows_int_literal(expr)
                    && !overflows_int_literal(left)
                    && !overflows_int_literal(right)
                {
                    self.report_int_overflow(expr);
                }

                let l_ty = self.infer_type(left, bind);
                let r_ty = self.infer_type(right, bind);

                let l_base_raw = base_type(&l_ty);
                let r_base_raw = base_type(&r_ty);
                let l_base_norm = normalize_for_binary(&l_base_raw);
                let r_base_norm = normalize_for_binary(&r_base_raw);
                let l_base = &l_base_norm;
                let r_base = &r_base_norm;
                let is_type_param_b = |t: &Type, checker: &Checker| matches!(&t.0, varn_core::TypeKind::Named(n, _) if checker.active_type_params.contains(n.as_ref()));
                if !l_base.is_dynamic()
                    && !r_base.is_dynamic()
                    && !is_type_param_b(l_base, self)
                    && !is_type_param_b(r_base, self)
                {
                    let is_numeric = |t: &Type| {
                        matches!(
                            &t.0,
                            TypeKind::Intrinsic(TypeTag::Int)
                                | TypeKind::Intrinsic(TypeTag::Float)
                                | TypeKind::Intrinsic(TypeTag::Decimal)
                                | TypeKind::Intrinsic(TypeTag::BigInt)
                        ) || matches!(&t.0, TypeKind::Named(n, _) if n.as_ref() == IntrinsicType::Decimal.as_str())
                    };
                    let same_numeric = is_numeric(l_base) && is_numeric(r_base);
                    let valid = match op {
                        BinaryOp::Add => {
                            same_numeric || l_base == &Type::Str || r_base == &Type::Str
                        }
                        BinaryOp::Sub
                        | BinaryOp::Mul
                        | BinaryOp::Div
                        | BinaryOp::Mod
                        | BinaryOp::Pow => same_numeric,
                        BinaryOp::BitAnd
                        | BinaryOp::BitOr
                        | BinaryOp::BitXor
                        | BinaryOp::Shl
                        | BinaryOp::Shr
                        | BinaryOp::UShr => l_base == &Type::Int && r_base == &Type::Int,
                        BinaryOp::Lt | BinaryOp::Gt | BinaryOp::LtEq | BinaryOp::GtEq => {
                            same_numeric || (l_base == &Type::Str && r_base == &Type::Str)
                        }
                        _ => true,
                    };
                    if !valid {
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::InvalidTypeOperator,
                                format!(
                                    "invalid binary operation '{}' between '{}' and '{}'",
                                    op_str(op),
                                    l_ty,
                                    r_ty
                                ),
                            )
                            .with_range(expr.range),
                        );
                    }
                }
            }
            ExprKind::Logical { left, right, .. } => {
                self.check_expr(left, bind);
                self.check_expr(right, bind);
            }
            ExprKind::Assign { target, value, .. } => {
                let prev = self.is_assignment_target;
                self.is_assignment_target = true;
                self.check_expr(target, bind);
                self.is_assignment_target = prev;
                self.check_expr(value, bind);

                self.check_extension_assignment(target, bind);

                if !matches!(
                    &target.kind,
                    ExprKind::Identifier { .. } | ExprKind::Member { .. }
                ) {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::NotAssignable,
                            "invalid left-hand side in assignment",
                        )
                        .with_range(target.range),
                    );
                }

                if let ExprKind::Identifier { name } = &target.kind {
                    let scope = bind.scopes.get(self.current_scope);
                    if let Some(id) = scope.resolve(name.as_ref(), &bind.scopes) {
                        let sym = bind.arena.get(id);
                        if sym.kind == crate::symbol::SymbolKind::Const {
                            self.emit(
                                Diagnostic::error(
                                    ErrorCode::NotAssignable,
                                    format!("cannot reassign to constant '{name}'"),
                                )
                                .with_range(expr.range),
                            );
                        }
                    }
                }

                let target_ty = if let ExprKind::Identifier { name } = &target.kind {
                    let scope = bind.scopes.get(self.current_scope);
                    scope
                        .resolve(name.as_ref(), &bind.scopes)
                        .and_then(|id| {
                            self.symbol_types
                                .get(&id)
                                .cloned()
                                .or_else(|| bind.arena.get(id).ty.clone())
                        })
                        .unwrap_or_else(|| self.infer_type(target, bind))
                } else {
                    self.infer_type(target, bind)
                };
                let value_ty = self.infer_type(value, bind);
                let is_empty_array_val = value_ty.is_dynamic()
                    && matches!(&value.kind, ExprKind::Array { elements } if elements.is_empty());
                if !is_empty_array_val
                    && !self.types_compatible_cached(&target_ty, &value_ty, Some(bind))
                {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::TypeMismatch,
                            format!("type mismatch: cannot assign '{value_ty}' to '{target_ty}'"),
                        )
                        .with_range(expr.range),
                    );
                }
            }
            ExprKind::Call {
                callee,
                args,
                type_args,
                ..
            } => self.check_call_expr(callee, args, type_args, &expr.range, expr.id, bind),
            ExprKind::New { callee, args, .. } => {
                if let ExprKind::Identifier { name: cls_name } = &callee.kind {
                    if self.abstract_classes.contains(cls_name.as_ref()) {
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::AbstractMethodNotImplemented,
                                format!("cannot instantiate abstract class '{cls_name}'"),
                            )
                            .with_range(expr.range),
                        );
                    }
                }
                self.check_expr(callee, bind);
                for arg in args {
                    match arg {
                        varn_core::ast::Arg::Positional(e) => self.check_expr(e, bind),
                        varn_core::ast::Arg::Named { value, .. } => self.check_expr(value, bind),
                        varn_core::ast::Arg::Spread(e) => self.check_expr(e, bind),
                    }
                }
            }

            ExprKind::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.check_expr(test, bind);
                self.check_expr(consequent, bind);
                self.check_expr(alternate, bind);
            }

            ExprKind::Member {
                object,
                property,
                computed,
                optional,
            } => self.check_member_expr(
                expr,
                object,
                property,
                *computed,
                *optional,
                &expr.range,
                bind,
            ),

            ExprKind::Paren { expression } => self.check_expr(expression, bind),
            ExprKind::NonNull { expression } => self.check_expr(expression, bind),

            ExprKind::Array { elements } => self.check_array_with_context(elements, bind),

            ExprKind::Tuple { elements } => {
                for e in elements {
                    self.check_expr(e, bind);
                }
            }

            ExprKind::Object { properties } => self.check_object_with_context(properties, bind),
            ExprKind::Record { properties } => self.check_object_with_context(properties, bind),

            ExprKind::Template { parts } => {
                for p in parts {
                    if let TemplatePart::Interpolation(e) = p {
                        self.check_expr(e, bind);
                    }
                }
            }

            ExprKind::Sequence { expressions } => {
                for e in expressions {
                    self.check_expr(e, bind);
                }
            }

            ExprKind::ClassExpr { declaration } => {
                self.check_decl(&varn_core::ast::Decl::Class((**declaration).clone()), bind);
            }

            ExprKind::Match { subject, cases } => {
                self.check_expr(subject, bind);
                let disc_narrowings = self.collect_match_disc_narrowings(subject, bind);
                for case in cases {
                    let saved_scope = self.current_scope;
                    if let Some(arm_scope) = self.next_child_scope(bind) {
                        self.current_scope = arm_scope;
                    }

                    if let Some(g) = &case.guard {
                        self.check_expr(g, bind);
                    }

                    let arm_disc_ty = match &case.pattern {
                        MatchPattern::Literal(e) => match &e.kind {
                            ExprKind::StrLiteral { .. } => Some(crate::types::Type::Str),
                            ExprKind::IntLiteral { .. } => Some(crate::types::Type::Int),
                            _ => None,
                        },
                        _ => None,
                    };

                    let narrowings = arm_disc_ty.and_then(|disc_ty| {
                        disc_narrowings.as_ref().map(|(id, members)| {
                            let matched: Vec<crate::types::Type> = members
                                .iter()
                                .filter(|m| {
                                    self.union_member_matches_disc(
                                        m,
                                        disc_narrowings.as_ref().map(|(_, _)| &disc_ty),
                                        disc_narrowings.as_ref().map(|(_, _)| subject.as_ref()),
                                        bind,
                                    )
                                })
                                .cloned()
                                .collect();
                            (*id, matched)
                        })
                    });

                    let narrowing_vec: Vec<(crate::symbol::SymbolId, crate::types::Type)> =
                        if let Some((id, matched)) = narrowings {
                            match matched.len() {
                                0 => vec![],
                                1 => vec![(id, matched.into_iter().next().unwrap())],
                                _ => vec![(id, crate::types::Type::union(matched))],
                            }
                        } else {
                            vec![]
                        };

                    self.with_narrowings(&narrowing_vec, |checker| {
                        if let Some(g) = &case.guard {
                            checker.check_expr(g, bind);
                        }

                        let subject_ty = checker.infer_type(subject, bind);
                        checker.check_pattern_match(&case.pattern, &subject_ty, bind);

                        match &case.body {
                            MatchBody::Expr(e) => checker.check_expr(e, bind),
                            MatchBody::Block(stmt) => checker.check_stmt(stmt, bind),
                        }
                    });
                    self.current_scope = saved_scope;
                }
                let subject_ty = self.infer_type(subject, bind);
                self.check_match_exhaustiveness(&subject_ty, cases, &expr.range, bind);
            }

            ExprKind::Update { operand, .. } => {
                self.check_expr(operand, bind);
                if !matches!(
                    &operand.kind,
                    ExprKind::Identifier { .. } | ExprKind::Member { .. }
                ) {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::NotAssignable,
                            "invalid left-hand side in update expression",
                        )
                        .with_range(operand.range),
                    );
                }
            }
            ExprKind::Spread { argument } => self.check_expr(argument, bind),

            ExprKind::Pipeline { left, right } => {
                self.check_expr(left, bind);
                let lhs_ty = self.infer_type(left, bind);
                let saved_pipeline = self.in_pipeline_rhs;
                let saved_pipe_ty = self.pipeline_value_type.replace(lhs_ty);
                self.in_pipeline_rhs = true;
                self.check_expr(right, bind);
                self.in_pipeline_rhs = saved_pipeline;
                self.pipeline_value_type = saved_pipe_ty;
            }

            ExprKind::Range { start, end, .. } => {
                self.check_expr(start, bind);
                self.check_expr(end, bind);
            }

            ExprKind::TaggedTemplate { tag, template, .. } => {
                self.check_expr(tag, bind);
                self.check_expr(template, bind);
                let tag_ty = self.infer_type(tag, bind).non_nullified();
                if let TypeKind::Fn(ft) = &tag_ty.0 {
                    self.record_type(expr.range.start.offset, ft.return_type.as_ref().clone());
                }
            }

            ExprKind::With { object, properties } => {
                self.check_expr(object, bind);
                for prop in properties {
                    match prop {
                        varn_core::ast::ObjectProp::Property { value, .. } => {
                            self.check_expr(value, bind);
                        }
                        varn_core::ast::ObjectProp::Spread { argument, .. } => {
                            self.check_expr(argument, bind);
                        }
                        _ => {}
                    }
                }
                let obj_ty = self.infer_type(object, bind);
                self.record_type(expr.range.start.offset, obj_ty);
            }

            ExprKind::MetaAccess { target, .. } => {
                self.check_expr(target, bind);
                let ty = self.infer_type(expr, bind);
                self.record_type(expr.range.start.offset, ty);
            }

            ExprKind::Identifier { name } => {
                if name.as_ref() == "_" {
                    if !self.is_assignment_target && !self.in_pipeline_rhs {
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::UnknownSymbol,
                                "cannot use '_' as a value; '_' is the discard placeholder",
                            )
                            .with_range(expr.range),
                        );
                    } else if self.in_pipeline_rhs {
                        // `_` stands for the piped value; record its concrete type so
                        // downstream consumers (compiler, LSP) don't see `dynamic`.
                        if let Some(ty) = self.pipeline_value_type.clone() {
                            self.record_type(expr.range.start.offset, ty);
                        }
                    }
                    return;
                }

                let scope = bind.scopes.get(self.current_scope);
                if scope.resolve(name.as_ref(), &bind.scopes).is_none()
                    && !self.is_assignment_target
                {
                    let mut diag = Diagnostic::error(
                        ErrorCode::UnknownSymbol,
                        format!("undefined variable: {name}"),
                    )
                    .with_range(expr.range);
                    if let Some(candidate) = closest_name(name.as_ref(), scope, &bind.scopes) {
                        diag =
                            diag.with_suggestion(Suggestion::did_you_mean(&candidate, expr.range));
                    }
                    self.emit(diag);
                }
            }

            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
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

fn closest_name(
    name: &str,
    scope: &crate::scope::CheckerScope,
    arena: &crate::scope::ScopeArena,
) -> Option<String> {
    let mut all: Vec<String> = Vec::new();
    let mut current = scope;
    loop {
        all.extend(current.bindings.keys().map(|k| k.to_string()));
        match current.parent {
            Some(parent_id) => current = arena.get(parent_id),
            None => break,
        }
    }
    let all_rc: Vec<Rc<str>> = all.into_iter().map(Rc::from).collect();
    closest_in_list(name, &all_rc).map(|s| s.to_owned())
}

/// What an expression is, for the literal-overflow diagnostic.
///
/// Three states, not two: an expression this cannot evaluate and one whose
/// value does not fit are DIFFERENT answers, and collapsing them into `None`
/// is how `2 ** 46` — which fits comfortably — got reported as overflow.
#[derive(PartialEq)]
enum ConstInt {
    /// An integer-literal expression with this value.
    Value(i64),
    /// An integer-literal expression whose value leaves the `int` range.
    Overflow,
    /// Not an integer-literal expression at all (an identifier, a call, a
    /// float, an operator this does not evaluate).
    NotConst,
}

/// Evaluate an integer-literal expression.
///
/// Deliberately narrow: literals, unary `+`/`-`, and the arithmetic operators,
/// all evaluated with the same `varn_core` checks the interpreter uses. It does
/// NOT follow `const` bindings or fold across statements — that is constant
/// propagation, which the SSA pipeline already owns. The point is to catch the
/// expression a person can read and see is out of range, where they wrote it.
fn const_int_expr(e: &Expr) -> ConstInt {
    use ConstInt::*;
    let lift = |o: Option<i64>| o.map_or(Overflow, Value);
    match &e.kind {
        // Every `i64` is a valid `int`, so a literal the parser produced is
        // always in range; a number too large to be an `i64` never reaches
        // here as an `IntLiteral`.
        ExprKind::IntLiteral { value, .. } => Value(*value),
        ExprKind::Unary { op, operand, .. } => {
            use varn_core::ast::operators::UnaryOp;
            let v = match const_int_expr(operand) {
                Value(v) => v,
                other => return other,
            };
            match op {
                UnaryOp::Minus => lift(varn_core::neg_int(v)),
                UnaryOp::Plus => Value(v),
                _ => NotConst,
            }
        }
        // Parentheses are a node, not just syntax; without this `(1 << 47)`
        // and `-(-x)` read as NotConst.
        ExprKind::Paren { expression } => const_int_expr(expression),
        ExprKind::Binary { left, right, op } => {
            let a = match const_int_expr(left) {
                Value(v) => v,
                other => return other,
            };
            let b = match const_int_expr(right) {
                Value(v) => v,
                other => return other,
            };
            match op {
                BinaryOp::Add => lift(varn_core::add_int(a, b)),
                BinaryOp::Sub => lift(varn_core::sub_int(a, b)),
                BinaryOp::Mul => lift(varn_core::mul_int(a, b)),
                // A negative exponent raises at run time rather than
                // overflowing, and one past u32 cannot be a literal `int`
                // anyway; neither is this diagnostic's business.
                BinaryOp::Pow => match u32::try_from(b) {
                    Ok(e) => lift(varn_core::pow_int(a, e)),
                    Err(_) => NotConst,
                },
                _ => NotConst,
            }
        }
        _ => NotConst,
    }
}

/// Whether `e` is an integer-literal expression that does not fit `int`.
///
/// KNOWN GAP: a literal `-(-2^47)` is not reported here even though it does
/// overflow — the shape the parser produces for a doubly-negated literal does
/// not reach this evaluator. It is caught at run time instead (see
/// `tests/errors/int-overflow-negate.vn`), which is the arm that matters for
/// safety; this diagnostic is an earlier warning, not the guarantee.
fn overflows_int_literal(e: &Expr) -> bool {
    const_int_expr(e) == ConstInt::Overflow
}
