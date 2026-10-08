use super::Checker;
use varn_core::ast::ExprId;
use varn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(super) fn check_match(
        &mut self,
        subject: ExprId,
        cases: &[varn_core::ast::MatchCase],
        expr: ExprId,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        let arena = self.ast_arena;
        self.check_expr(subject, bind);
        let disc_narrowings = self.collect_match_disc_narrowings(subject, bind);
        let base_subject_ty = self.infer_type(subject, bind);
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

            if let Some(g) = &case.guard {
                self.check_expr(*g, bind);
            }

            let arm_disc_ty = match &case.pattern {
                varn_core::ast::MatchPattern::Literal(e) => match &arena.expr(*e).kind {
                    varn_core::ast::ExprKind::StrLiteral { .. } => Some(varn_sem::types::Type::Str),
                    varn_core::ast::ExprKind::IntLiteral { .. } => Some(varn_sem::types::Type::Int),
                    varn_core::ast::ExprKind::FloatLiteral { .. }
                    | varn_core::ast::ExprKind::BigIntLiteral { .. }
                    | varn_core::ast::ExprKind::DecimalLiteral { .. }
                    | varn_core::ast::ExprKind::CharLiteral { .. }
                    | varn_core::ast::ExprKind::BoolLiteral { .. }
                    | varn_core::ast::ExprKind::NullLiteral
                    | varn_core::ast::ExprKind::RegexLiteral { .. }
                    | varn_core::ast::ExprKind::Template { .. }
                    | varn_core::ast::ExprKind::TaggedTemplate { .. }
                    | varn_core::ast::ExprKind::Identifier { .. }
                    | varn_core::ast::ExprKind::Missing
                    | varn_core::ast::ExprKind::This
                    | varn_core::ast::ExprKind::Super
                    | varn_core::ast::ExprKind::Array { .. }
                    | varn_core::ast::ExprKind::Object { .. }
                    | varn_core::ast::ExprKind::Tuple { .. }
                    | varn_core::ast::ExprKind::Record { .. }
                    | varn_core::ast::ExprKind::Unary { .. }
                    | varn_core::ast::ExprKind::Update { .. }
                    | varn_core::ast::ExprKind::Binary { .. }
                    | varn_core::ast::ExprKind::Logical { .. }
                    | varn_core::ast::ExprKind::Assign { .. }
                    | varn_core::ast::ExprKind::Conditional { .. }
                    | varn_core::ast::ExprKind::Member { .. }
                    | varn_core::ast::ExprKind::Call { .. }
                    | varn_core::ast::ExprKind::New { .. }
                    | varn_core::ast::ExprKind::Function { .. }
                    | varn_core::ast::ExprKind::Arrow { .. }
                    | varn_core::ast::ExprKind::Sequence { .. }
                    | varn_core::ast::ExprKind::Paren { .. }
                    | varn_core::ast::ExprKind::Await { .. }
                    | varn_core::ast::ExprKind::Spawn { .. }
                    | varn_core::ast::ExprKind::Yield { .. }
                    | varn_core::ast::ExprKind::Spread { .. }
                    | varn_core::ast::ExprKind::Pipeline { .. }
                    | varn_core::ast::ExprKind::Range { .. }
                    | varn_core::ast::ExprKind::NonNull { .. }
                    | varn_core::ast::ExprKind::Try { .. }
                    | varn_core::ast::ExprKind::As { .. }
                    | varn_core::ast::ExprKind::Satisfies { .. }
                    | varn_core::ast::ExprKind::ClassExpr { .. }
                    | varn_core::ast::ExprKind::Match { .. }
                    | varn_core::ast::ExprKind::Is { .. }
                    | varn_core::ast::ExprKind::With { .. }
                    | varn_core::ast::ExprKind::MetaAccess { .. } => None,
                },
                varn_core::ast::MatchPattern::Wildcard
                | varn_core::ast::MatchPattern::Identifier(_)
                | varn_core::ast::MatchPattern::Record { .. }
                | varn_core::ast::MatchPattern::Sequence(_)
                | varn_core::ast::MatchPattern::Type { .. }
                | varn_core::ast::MatchPattern::EnumVariant { .. } => None,
            };

            let narrowings = arm_disc_ty.and_then(|disc_ty| {
                disc_narrowings.as_ref().map(|(id, members)| {
                    let matched: Vec<varn_sem::types::Type> = members
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

            let narrowing_vec: Vec<(varn_sem::symbol::SymbolId, varn_sem::types::Type)> =
                if let Some((id, matched)) = narrowings {
                    match matched.len() {
                        0 => vec![],
                        1 => vec![(id, matched.into_iter().next().unwrap())],
                        _ => vec![(
                            id,
                            varn_sem::types::Type::union(
                                matched,
                                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                            ),
                        )],
                    }
                } else {
                    vec![]
                };

            self.with_narrowings(&narrowing_vec, |checker| {
                if let Some(g) = &case.guard {
                    checker.check_expr(*g, bind);
                }

                let subject_ty = checker.infer_type(subject, bind);
                let subject_ty = if null_handled {
                    subject_ty.non_nullified(std::sync::Arc::make_mut(&mut checker.ty_table))
                } else {
                    subject_ty
                };
                checker.check_pattern_match(&case.pattern, &subject_ty, bind);

                match &case.body {
                    varn_core::ast::MatchBody::Expr(e) => checker.check_expr(*e, bind),
                    varn_core::ast::MatchBody::Block(stmt) => checker.check_stmt(*stmt, bind),
                }
            });
            self.current_scope = saved_scope;
            null_handled |= case.guard.is_none()
                && matches!(
                    &case.pattern,
                    varn_core::ast::MatchPattern::Literal(e)
                        if matches!(arena.expr(*e).kind, varn_core::ast::ExprKind::NullLiteral)
                );
        }
        self.desugar
            .match_arm_subjects
            .insert(subject.index(), arm_subjects);
        let subject_ty = self.infer_type(subject, bind);
        self.check_match_exhaustiveness(expr, &subject_ty, cases, &range, bind);
    }
}
