use super::context::FnEmitter;
use rustc_hash::FxHashMap;
use varn_core::ast::ExprId;
use varn_tir::{BackendTy, Resolution, TirBinOp, TirExpr, TirExprKind, TirStmt};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_switch(
        &mut self,
        discriminant: ExprId,
        cases: &[varn_core::ast::SwitchCase],
    ) -> Vec<TirStmt> {
        let d = self.lower_expr(discriminant);
        let mut out = std::mem::take(&mut self.pending);
        let d = self.hoist(d);
        out.extend(std::mem::take(&mut self.pending));
        out.extend(self.switch_cases(&d, cases, 0));
        out
    }

    pub(super) fn switch_cases(
        &mut self,
        d: &TirExpr,
        cases: &[varn_core::ast::SwitchCase],
        i: usize,
    ) -> Vec<TirStmt> {
        let Some(case) = cases.get(i) else {
            return vec![];
        };
        self.scopes.push(FxHashMap::default());
        let body: Vec<TirStmt> = case.body.iter().flat_map(|&s| self.lower_stmt(s)).collect();
        self.scopes.pop();
        let rest = self.switch_cases(d, cases, i + 1);
        match case.test {
            None => {
                let mut v = body;
                v.extend(rest);
                v
            }
            Some(t) => {
                let te = self.lower_expr(t);
                let mut pre = std::mem::take(&mut self.pending);
                let cond = TirExpr {
                    kind: TirExprKind::Binary {
                        op: TirBinOp::Eq,
                        lhs: Box::new(d.clone()),
                        rhs: Box::new(te),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span: d.span,
                };
                pre.push(TirStmt::If {
                    cond,
                    then_body: body,
                    else_body: rest,
                });
                pre
            }
        }
    }
}
