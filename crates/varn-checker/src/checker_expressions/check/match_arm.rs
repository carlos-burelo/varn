use super::Checker;
use crate::binder::BindResult;
use varn_core::ast::ExprId;

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
        for case in cases {
            let saved_scope = self.current_scope;
            if let Some(arm_scope) = self.next_child_scope(bind) {
                self.current_scope = arm_scope;
            }

            if let Some(g) = &case.guard {
                self.check_expr(*g, bind);
            }

            let arm_disc_ty = match &case.pattern {
                varn_core::ast::MatchPattern::Literal(e) => match &arena.expr(*e).kind {
                    varn_core::ast::ExprKind::StrLiteral { .. } => Some(crate::types::Type::Str),
                    varn_core::ast::ExprKind::IntLiteral { .. } => Some(crate::types::Type::Int),
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
                                disc_narrowings.as_ref().map(|(_, _)| subject),
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
                        _ => vec![(
                            id,
                            crate::types::Type::union(
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
                checker.check_pattern_match(&case.pattern, &subject_ty, bind);

                match &case.body {
                    varn_core::ast::MatchBody::Expr(e) => checker.check_expr(*e, bind),
                    varn_core::ast::MatchBody::Block(stmt) => checker.check_stmt(*stmt, bind),
                }
            });
            self.current_scope = saved_scope;
        }
        let subject_ty = self.infer_type(subject, bind);
        self.check_match_exhaustiveness(expr, &subject_ty, cases, &range, bind);
    }
}
