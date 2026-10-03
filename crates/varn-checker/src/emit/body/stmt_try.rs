use super::context::FnEmitter;
use super::finally_splice;
use super::small_utils::bool_lit;
use crate::emit::ty::NameResolver;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::{Pattern, StmtId};
use varn_tir::{BackendTy, DynReason, Resolution, Span, TirBinOp, TirExpr, TirExprKind, TirStmt};

impl<'a> FnEmitter<'a> {
    pub(super) fn catch_type_names(&self, t: &varn_core::ast::TypeNode) -> Vec<Arc<str>> {
        use varn_core::TypeKind;
        match &t.kind {
            TypeKind::Named(n, _) => vec![Arc::from(self.m.interner.resolve(*n))],
            TypeKind::Union(items) | TypeKind::Intersection(items) => items
                .iter()
                .flat_map(|x| self.catch_type_names(x))
                .collect(),
            _ => vec![],
        }
    }

    pub(super) fn instance_of_name(&mut self, value: TirExpr, name: &str) -> TirExpr {
        let span = value.span;
        if let Some(class) = self.m.names.class_id(name) {
            return TirExpr {
                kind: TirExprKind::TypeTest {
                    value: Box::new(value),
                    class,
                },
                ty: BackendTy::Bool,
                res: Resolution::None,
                span,
            };
        }
        let rhs = TirExpr {
            kind: TirExprKind::Var,
            ty: BackendTy::Dynamic(DynReason::NotYetSupported),
            res: self.resolve_name(name),
            span,
        };
        TirExpr {
            kind: TirExprKind::Binary {
                op: TirBinOp::Instanceof,
                lhs: Box::new(value),
                rhs: Box::new(rhs),
            },
            ty: BackendTy::Bool,
            res: Resolution::None,
            span,
        }
    }

    pub(super) fn lower_try(
        &mut self,
        block: StmtId,
        catches: &[varn_core::ast::CatchClause],
        finally: Option<StmtId>,
    ) -> Vec<TirStmt> {
        let body = self.lower_stmt_as_block(block);
        let dyn_ty = BackendTy::Dynamic(DynReason::NotYetSupported);

        self.scopes.push(FxHashMap::default());
        let catch_local = self.bind_local(Arc::from("<catch>"), dyn_ty);
        let e_var = |span: Span| TirExpr {
            kind: TirExprKind::Var,
            ty: dyn_ty,
            res: Resolution::Local(catch_local),
            span,
        };
        let lower_clause = |this: &mut Self, c: &varn_core::ast::CatchClause| -> Vec<TirStmt> {
            this.scopes.push(FxHashMap::default());
            let mut out = Vec::new();
            if let Some(Pattern::Identifier { name, .. }) = &c.param {
                if this.m.interner.resolve(*name) != "<catch>" {
                    let alias = this.bind_local(Arc::from(this.m.interner.resolve(*name)), dyn_ty);
                    out.push(TirStmt::Let {
                        local: alias,
                        ty: dyn_ty,
                        init: Some(e_var(Span::EMPTY)),
                    });
                }
            }
            out.extend(this.lower_stmt_as_block(c.body));
            this.scopes.pop();
            out
        };
        let typed: Vec<&varn_core::ast::CatchClause> =
            catches.iter().filter(|c| c.type_ann.is_some()).collect();
        let catch_all = catches.iter().find(|c| c.type_ann.is_none());
        let mut chain: Vec<TirStmt> = match catch_all {
            Some(c) => lower_clause(self, c),
            None => vec![TirStmt::Throw(e_var(Span::EMPTY))],
        };
        for c in typed.iter().rev() {
            let names = self.catch_type_names(c.type_ann.as_ref().unwrap());
            let cond = names
                .into_iter()
                .map(|n| self.instance_of_name(e_var(Span::EMPTY), &n))
                .reduce(|a, b| TirExpr {
                    kind: TirExprKind::Select {
                        cond: Box::new(a),
                        then_val: Box::new(bool_lit(true)),
                        else_val: Box::new(b),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span: Span::EMPTY,
                })
                .unwrap_or_else(|| bool_lit(true));
            let then_body = lower_clause(self, c);
            chain = vec![TirStmt::If {
                cond,
                then_body,
                else_body: std::mem::take(&mut chain),
            }];
        }
        self.scopes.pop();
        let catch_body = chain;

        let fin: Vec<TirStmt> = finally
            .map(|f| self.lower_stmt_as_block(f))
            .unwrap_or_default();
        let (body, catch_body) = if fin.is_empty() {
            (body, catch_body)
        } else {
            (
                finally_splice::splice_finally_before_exits(body, &fin),
                finally_splice::splice_finally_before_exits_and_throw(catch_body, &fin),
            )
        };
        let mut out = vec![TirStmt::Try {
            body,
            catch_local,
            catch_body,
        }];
        out.extend(fin);
        out
    }
}
