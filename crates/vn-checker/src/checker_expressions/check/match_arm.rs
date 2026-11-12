use super::Checker;
use crate::checker::recorder::Recorder;
use vn_core::ast::ExprId;
use vn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(super) fn check_match(
        &mut self,
        rec: &mut Recorder,
        subject: ExprId,
        cases: &[vn_core::ast::MatchCase],
        expr: ExprId,
        range: vn_core::SourceRange,
        bind: &BindResult,
    ) {
        let arena = self.ast_arena;
        self.check_expr(rec, subject, bind);
        let disc_narrowings = self.collect_match_disc_narrowings(rec, subject, bind);
        let base_subject_ty = self.infer_type(rec, subject, bind);
        let mut arm_subjects = Vec::with_capacity(cases.len());
        let mut null_handled = false;
        for case in cases {
            arm_subjects.push(if null_handled {
                base_subject_ty.non_nullified(std::sync::Arc::make_mut(&mut self.ty_table))
            } else {
                base_subject_ty
            });
            let saved_scope = self.current_scope;
            if let Some(arm_scope) = self.next_child_scope(bind) {
                self.current_scope = arm_scope;
            }

            let arm_disc_ty = match &case.pattern {
                vn_core::ast::MatchPattern::Literal(e) => match &arena.expr(*e).kind {
                    vn_core::ast::ExprKind::StrLiteral { .. } => Some(vn_sem::types::Type::Str),
                    vn_core::ast::ExprKind::IntLiteral { .. } => Some(vn_sem::types::Type::Int),
                    vn_core::ast::ExprKind::FloatLiteral { .. }
                    | vn_core::ast::ExprKind::BigIntLiteral { .. }
                    | vn_core::ast::ExprKind::DecimalLiteral { .. }
                    | vn_core::ast::ExprKind::CharLiteral { .. }
                    | vn_core::ast::ExprKind::BoolLiteral { .. }
                    | vn_core::ast::ExprKind::NullLiteral
                    | vn_core::ast::ExprKind::Template { .. }
                    | vn_core::ast::ExprKind::TaggedTemplate { .. }
                    | vn_core::ast::ExprKind::Identifier { .. }
                    | vn_core::ast::ExprKind::Missing
                    | vn_core::ast::ExprKind::This
                    | vn_core::ast::ExprKind::ImportMeta(_)
                    | vn_core::ast::ExprKind::Super
                    | vn_core::ast::ExprKind::Array { .. }
                    | vn_core::ast::ExprKind::Object { .. }
                    | vn_core::ast::ExprKind::Tuple { .. }
                    | vn_core::ast::ExprKind::Unary { .. }
                    | vn_core::ast::ExprKind::Update { .. }
                    | vn_core::ast::ExprKind::Binary { .. }
                    | vn_core::ast::ExprKind::Logical { .. }
                    | vn_core::ast::ExprKind::Assign { .. }
                    | vn_core::ast::ExprKind::Conditional { .. }
                    | vn_core::ast::ExprKind::Member { .. }
                    | vn_core::ast::ExprKind::Call { .. }
                    | vn_core::ast::ExprKind::New { .. }
                    | vn_core::ast::ExprKind::Function { .. }
                    | vn_core::ast::ExprKind::Arrow { .. }
                    | vn_core::ast::ExprKind::Sequence { .. }
                    | vn_core::ast::ExprKind::Paren { .. }
                    | vn_core::ast::ExprKind::Await { .. }
                    | vn_core::ast::ExprKind::Spawn { .. }
                    | vn_core::ast::ExprKind::Yield { .. }
                    | vn_core::ast::ExprKind::Spread { .. }
                    | vn_core::ast::ExprKind::Pipeline { .. }
                    | vn_core::ast::ExprKind::Range { .. }
                    | vn_core::ast::ExprKind::NonNull { .. }
                    | vn_core::ast::ExprKind::Try { .. }
                    | vn_core::ast::ExprKind::As { .. }
                    | vn_core::ast::ExprKind::Satisfies { .. }
                    | vn_core::ast::ExprKind::ClassExpr { .. }
                    | vn_core::ast::ExprKind::Match { .. }
                    | vn_core::ast::ExprKind::Is { .. }
                    | vn_core::ast::ExprKind::With { .. }
                    | vn_core::ast::ExprKind::MetaAccess { .. } => None,
                },
                vn_core::ast::MatchPattern::Wildcard
                | vn_core::ast::MatchPattern::Identifier(_)
                | vn_core::ast::MatchPattern::Tuple(_)
                | vn_core::ast::MatchPattern::Array { .. }
                | vn_core::ast::MatchPattern::Record { .. }
                | vn_core::ast::MatchPattern::EnumVariant { .. } => None,
            };

            let narrowings = arm_disc_ty.and_then(|disc_ty| {
                disc_narrowings.as_ref().map(|(id, members)| {
                    let matched: Vec<vn_sem::types::Type> = members
                        .iter()
                        .filter(|m| {
                            self.union_member_matches_disc(
                                m,
                                disc_narrowings.as_ref().map(|(_, _)| &disc_ty),
                                disc_narrowings.as_ref().map(|(_, _)| subject),
                                bind,
                            )
                        })
                        .cloned()
                        .collect();
                    (*id, matched)
                })
            });

            let narrowing_vec: Vec<(vn_sem::symbol::SymbolId, vn_sem::types::Type)> =
                if let Some((id, matched)) = narrowings {
                    match matched.len() {
                        0 => vec![],
                        1 => vec![(id, matched.into_iter().next().unwrap())],
                        _ => vec![(
                            id,
                            vn_sem::types::Type::union(
                                matched,
                                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                            ),
                        )],
                    }
                } else {
                    vec![]
                };

            self.with_narrowings(&narrowing_vec, |checker| {
                let subject_ty = checker.infer_type(rec, subject, bind);
                let subject_ty = if null_handled {
                    subject_ty.non_nullified(std::sync::Arc::make_mut(&mut checker.ty_table))
                } else {
                    subject_ty
                };
                checker.check_pattern_match(rec, &case.pattern, &subject_ty, case.range, bind);
                let guard_narrowings = if let Some(g) = case.guard {
                    checker.check_expr(rec, g, bind);
                    checker.require_condition(rec, g, bind);
                    if checker.can_extract_narrowings(g) {
                        checker.extract_narrowings(rec, g, bind, true)
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };
                checker.with_narrowings(&guard_narrowings, |checker| match &case.body {
                    vn_core::ast::MatchBody::Expr(e) => checker.check_expr(rec, *e, bind),
                    vn_core::ast::MatchBody::Block(stmt) => checker.check_stmt(rec, *stmt, bind),
                });
            });
            self.current_scope = saved_scope;
            null_handled |= case.guard.is_none()
                && matches!(
                    &case.pattern,
                    vn_core::ast::MatchPattern::Literal(e)
                        if matches!(arena.expr(*e).kind, vn_core::ast::ExprKind::NullLiteral)
                );
        }
        rec.desugar
            .match_arm_subjects
            .insert(subject.index(), arm_subjects);
        let subject_ty = self.infer_type(rec, subject, bind);
        self.check_match_exhaustiveness(rec, expr, &subject_ty, cases, &range, bind);
    }
}
