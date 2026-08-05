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

impl Checker {
    pub(crate) fn check_expr(&mut self, expr: &Expr, bind: &BindResult) {
        self.check_expr_no_record(expr, bind);
        let start = expr.range.start.offset;
        let end = expr.range.end.offset.saturating_sub(1);
        self.node_scopes.insert(expr.id, self.current_scope);
        let ty = self.infer_type(expr, bind);
        self.resolved_expr_types.insert(expr.id, ty.clone());

        let mut symbol_id = None;
        if let ExprKind::Identifier { name } = &expr.kind {
            let scope = bind.scopes.get(self.current_scope);
            symbol_id = scope.resolve(name.as_ref(), &bind.scopes);
        }

        if self.record_expr_types {
            self.expr_types
                .entry(start)
                .or_insert(crate::checker::ExprInfo {
                    ty: ty.clone(),
                    symbol_id,
                });

            if start != end {
                self.expr_types
                    .entry(end)
                    .or_insert(crate::checker::ExprInfo { ty, symbol_id });
            }
        }
    }

    fn check_expr_no_record(&mut self, expr: &Expr, bind: &BindResult) {
        match &expr.kind {
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
                    self.record_scope(expr.range.start.offset);
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
                    self.record_scope(body.range.start.offset);
                }

                let saved_in_function = self.in_function;
                let saved_loop_depth = self.loop_depth;
                let saved_switch_depth = self.switch_depth;
                self.in_function = true;
                self.loop_depth = 0;
                self.switch_depth = 0;

                self.check_stmt(body, bind);

                self.in_function = saved_in_function;
                self.loop_depth = saved_loop_depth;
                self.switch_depth = saved_switch_depth;

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
                if !arg_ty.is_dynamic()
                    && !matches!(&arg_ty.0, TypeKind::Generic(n, _, _) if n.as_ref() == IntrinsicType::Task.as_str() || n.as_ref() == IntrinsicType::TaskHandle.as_str())
                    && !matches!(&arg_ty.0, TypeKind::Intrinsic(TypeTag::Void))
                {
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
            ExprKind::Unary { operand, .. } => self.check_expr(operand, bind),
            ExprKind::Binary { left, right, op } => {
                self.check_expr(left, bind);
                self.check_expr(right, bind);
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
                                | TypeKind::LiteralInt(_)
                                | TypeKind::LiteralFloat(_)
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
            } => self.check_call_expr(callee, args, type_args, &expr.range, bind),
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
                            ExprKind::StrLiteral { value } => {
                                Some(crate::types::Type::literal_str(value.to_string()))
                            }
                            ExprKind::IntLiteral { value, .. } => {
                                Some(crate::types::Type::literal_int(*value))
                            }
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
                            let _ = g;
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
            | ExprKind::NullLiteral { .. }
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
