use super::helpers::{base_type, op_str};
use super::infer::member_binary::normalize_for_binary;
use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::Type;
use varn_core::ast::operators::BinaryOp;
use varn_core::ast::{Expr, ExprKind};
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode, TypeKind};

impl Checker {
    pub(super) fn check_binary_expr(
        &mut self,
        left: &Expr,
        right: &Expr,
        op: &BinaryOp,
        range: &SourceRange,
        bind: &BindResult,
    ) {
        self.check_expr(left, bind);
        self.check_expr(right, bind);
        let l_ty = self.infer_type(left, bind);
        let r_ty = self.infer_type(right, bind);

        let l_base_norm = normalize_for_binary(base_type(&l_ty));
        let r_base_norm = normalize_for_binary(base_type(&r_ty));
        let l_base = &l_base_norm;
        let r_base = &r_base_norm;

        if l_base.is_dynamic() || r_base.is_dynamic() {
            return;
        }
        
        let is_type_param = |t: &Type| matches!(&t.0, TypeKind::Named(n, _) if self.active_type_params.contains(n));
        if is_type_param(l_base) || is_type_param(r_base) {
            return;
        }

        let is_numeric = |t: &Type| {
            matches!(
                &t.0,
                TypeKind::Int
                    | TypeKind::Float
                    | TypeKind::Decimal
                    | TypeKind::BigInt
                    | TypeKind::LiteralInt(_)
                    | TypeKind::LiteralFloat(_)
            ) || matches!(&t.0, TypeKind::Named(n, _) if n == varn_core::IntrinsicType::Decimal.as_str())
        };
        let same_numeric_kind = is_numeric(l_base) && is_numeric(r_base);
        let valid = match op {
            BinaryOp::Add => {
                same_numeric_kind || l_base == &Type::Str || r_base == &Type::Str
            }
            BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Pow => {
                same_numeric_kind
            }
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr
            | BinaryOp::UShr => l_base == &Type::Int && r_base == &Type::Int,
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::LtEq | BinaryOp::GtEq => {
                same_numeric_kind || (l_base == &Type::Str && r_base == &Type::Str)
            }
            _ => true,
        };

        if !valid {
            self.emit(
                Diagnostic::error(ErrorCode::InvalidTypeOperator, format!(
                    "invalid binary operation '{}' between '{}' and '{}'",
                    op_str(op),
                    l_ty,
                    r_ty
                ))
                .with_range(*range),
            );
        }
    }

    pub(super) fn check_assign_expr(
        &mut self,
        target: &Expr,
        value: &Expr,
        range: &SourceRange,
        bind: &BindResult,
    ) {
        let prev = self.is_assignment_target;
        self.is_assignment_target = true;
        self.check_expr(target, bind);
        self.is_assignment_target = prev;
        self.check_expr(value, bind);

        self.check_extension_assignment(target, bind);

        if let ExprKind::Identifier { name } = &target.kind {
            let scope = bind.scopes.get(self.current_scope);
            if let Some(id) = scope.resolve(name, &bind.scopes) {
                let sym = bind.arena.get(id);
                if sym.kind == crate::symbol::SymbolKind::Const {
                    self.emit(
                        Diagnostic::error(ErrorCode::NotAssignable, format!("cannot reassign to constant '{name}'"))
                            .with_range(*range),
                    );
                }
            }
        }

        let target_ty = if let ExprKind::Identifier { name } = &target.kind {
            let scope = bind.scopes.get(self.current_scope);
            scope
                .resolve(name, &bind.scopes)
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
        if !is_empty_array_val && !self.types_compatible_cached(&target_ty, &value_ty, Some(bind)) {
            self.emit(
                Diagnostic::error(ErrorCode::TypeMismatch, format!("type mismatch: cannot assign '{value_ty}' to '{target_ty}'"))
                    .with_range(*range),
            );
        }
    }

    pub(super) fn check_match_expr(
        &mut self,
        subject: &Expr,
        cases: &[varn_core::ast::MatchCase],
        range: &SourceRange,
        bind: &BindResult,
    ) {
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
                varn_core::ast::MatchPattern::Literal(e) => {
                    self.check_expr(e, bind);
                    match &e.kind {
                        ExprKind::StrLiteral { value } => {
                            Some(crate::types::Type::literal_str(value.clone()))
                        }
                        ExprKind::IntLiteral { value, .. } => {
                            Some(crate::types::Type::literal_int(*value))
                        }
                        _ => None,
                    }
                }
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
                match &case.body {
                    varn_core::ast::MatchBody::Expr(e) => checker.check_expr(e, bind),
                    varn_core::ast::MatchBody::Block(stmt) => checker.check_stmt(stmt, bind),
                }
            });
            self.current_scope = saved_scope;
        }
        let subject_ty = self.infer_type(subject, bind);
        self.check_match_exhaustiveness(&subject_ty, cases, range, bind);
    }
}










