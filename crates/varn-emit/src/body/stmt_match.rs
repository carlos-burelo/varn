use super::context::FnEmitter;
use super::match_pattern::MatchDest;
use crate::ty::lower_type;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::{ExprId, MatchBody, MatchCase};
use varn_tir::{BackendTy, DynReason, Resolution, TirArg, TirExpr, TirExprKind, TirStmt};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_match(
        &mut self,
        subject: ExprId,
        cases: &[MatchCase],
        dest: MatchDest,
    ) -> Vec<TirStmt> {
        let subj = self.lower_expr(subject);
        let mut out = std::mem::take(&mut self.pending);
        let s = self.hoist(subj);
        out.extend(std::mem::take(&mut self.pending));
        let chain = self.match_cases(&s, subject, cases, 0, dest);
        out.extend(chain);
        out
    }

    pub(super) fn lower_match_stmt(
        &mut self,
        subject: ExprId,
        cases: &[MatchCase],
    ) -> Vec<TirStmt> {
        self.lower_match(subject, cases, MatchDest::Statement)
    }

    pub(super) fn lower_match_expr(
        &mut self,
        subject: ExprId,
        cases: &[MatchCase],
        ty: BackendTy,
        span: varn_tir::Span,
    ) -> TirExpr {
        let result = self.fresh_local(ty);
        self.pending.push(TirStmt::Let {
            local: result,
            ty,
            init: None,
        });
        let chain = self.lower_match(subject, cases, MatchDest::Assign(result));
        self.pending.extend(chain);
        TirExpr {
            kind: TirExprKind::Var,
            ty,
            res: Resolution::Local(result),
            span,
        }
    }

    pub(super) fn match_cases(
        &mut self,
        s: &TirExpr,
        subject: ExprId,
        cases: &[MatchCase],
        i: usize,
        dest: MatchDest,
    ) -> Vec<TirStmt> {
        let Some(case) = cases.get(i) else {
            return match dest {
                MatchDest::Statement => vec![],
                MatchDest::Return | MatchDest::Assign(_) => vec![self.no_match_arm(s.span)],
            };
        };

        self.scopes.push(FxHashMap::default());
        let arm_subject = self.arm_subject(s, subject, i);
        let (cond, bindings) = self.match_pattern(&arm_subject, &case.pattern);

        let guard = case.guard.map(|g| {
            let gexpr = self.lower_expr(g);
            let pending = std::mem::take(&mut self.pending);
            (pending, gexpr)
        });
        let mut then_body: Vec<TirStmt> = Vec::new();

        let value = match &case.body {
            MatchBody::Expr(e) => self.lower_expr(*e),
            MatchBody::Block(stmt) => {
                then_body.extend(self.lower_stmt_as_block(*stmt));
                TirExpr {
                    kind: TirExprKind::NullLit,
                    ty: BackendTy::Dynamic(DynReason::NotYetSupported),
                    res: Resolution::None,
                    span: s.span,
                }
            }
        };
        then_body.extend(std::mem::take(&mut self.pending));
        match dest {
            MatchDest::Statement => then_body.push(TirStmt::Expr(value)),
            MatchDest::Return => then_body.push(TirStmt::Return(Some(value))),
            MatchDest::Assign(local) => then_body.push(TirStmt::Expr(TirExpr {
                kind: TirExprKind::Assign {
                    target: Box::new(TirExpr {
                        kind: TirExprKind::Var,
                        ty: value.ty,
                        res: Resolution::Local(local),
                        span: s.span,
                    }),
                    value: Box::new(value),
                },
                ty: BackendTy::Void,
                res: Resolution::None,
                span: s.span,
            })),
        }
        self.scopes.pop();

        let else_body = self.match_cases(s, subject, cases, i + 1, dest);
        match guard {
            None => {
                let mut m = bindings;
                m.extend(then_body);
                vec![TirStmt::If {
                    cond,
                    then_body: m,
                    else_body,
                }]
            }
            Some((gpending, gexpr)) => {
                let mut matched = bindings;
                matched.extend(gpending);
                matched.push(TirStmt::If {
                    cond: gexpr,
                    then_body,
                    else_body: else_body.clone(),
                });
                vec![TirStmt::If {
                    cond,
                    then_body: matched,
                    else_body,
                }]
            }
        }
    }

    pub(super) fn no_match_arm(&mut self, span: varn_tir::Span) -> TirStmt {
        let class_name = varn_core::RuntimeErrorKind::MatchError.class_name();
        let callee = TirExpr {
            kind: TirExprKind::Var,
            ty: BackendTy::Dynamic(DynReason::NotYetSupported),
            res: self.resolve_name(class_name),
            span,
        };
        let message = TirExpr {
            kind: TirExprKind::StrLit(Arc::from("no match arm matched the value")),
            ty: BackendTy::Str,
            res: Resolution::None,
            span,
        };
        TirStmt::Throw(TirExpr {
            kind: TirExprKind::Call {
                callee: Box::new(callee),
                args: vec![TirArg::Expr(message)],
            },
            ty: BackendTy::Dynamic(DynReason::NotYetSupported),
            res: Resolution::None,
            span,
        })
    }

    fn arm_subject(&mut self, s: &TirExpr, subject: ExprId, arm: usize) -> TirExpr {
        let Some(checked) = self
            .m
            .desugar
            .match_arm_subjects
            .get(&subject.index())
            .and_then(|arms| arms.get(arm))
        else {
            return s.clone();
        };
        let ty = lower_type(
            checked,
            self.m.checker_table,
            self.m.interner,
            self.tt,
            self.m.names,
        );
        self.cast_to(s.clone(), ty)
    }
}
