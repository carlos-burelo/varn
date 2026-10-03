use super::context::FnEmitter;
use super::small_utils::pattern_lead;
use std::sync::Arc;
use varn_core::ast::{ExprId, StmtId};
use varn_tir::{
    BackendTy, DynReason, Resolution, SigId, Span, TirExpr, TirExprKind, TirFunction, TirStmt,
};

pub(super) enum ClosureBody {
    Expr(ExprId),
    Stmt(StmtId),
}

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_closure(
        &mut self,
        params: &[varn_core::ast::Param],
        body: ClosureBody,
        is_async: bool,
        is_generator: bool,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let func_id = varn_tir::FnId(self.closure_base + self.out_closures.len() as u32);

        self.out_closures.push(TirFunction {
            name: Arc::from("<closure>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Dynamic(DynReason::NotYetSupported),
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async,
            is_generator,
            has_rest: params.last().is_some_and(|p| p.is_rest),
        });
        let slot = self.out_closures.len() - 1;

        let param_names: Vec<Arc<str>> = params
            .iter()
            .map(|p| pattern_lead(&p.pattern, self.m.interner))
            .collect();
        let param_tys = vec![BackendTy::Dynamic(DynReason::NotYetSupported); params.len()];

        let mut outer_names = self.outer_names.clone();
        for scope in &self.scopes {
            outer_names.extend(scope.keys().cloned());
        }
        outer_names.extend(self.params.iter().cloned());

        let mut sub = FnEmitter::new(
            self.ast_arena,
            self.expr_table,
            &mut *self.tt,
            self.m,
            &mut *self.signatures,
            &mut *self.out_closures,
            self.closure_base,
            param_names,
        );
        sub.outer_names = outer_names;
        let mut stmts = sub.destructure_params(params);
        stmts.extend(match body {
            ClosureBody::Stmt(s) => sub.lower_stmt_as_block(s),
            ClosureBody::Expr(e) => {
                let te = sub.lower_expr(e);
                let mut b = std::mem::take(&mut sub.pending);
                b.push(TirStmt::Return(Some(te)));
                b
            }
        });
        let locals = sub.locals;
        let captures = std::mem::take(&mut sub.captures);

        self.out_closures[slot] = TirFunction {
            name: Arc::from("<closure>"),
            sig: SigId(0),
            params: param_tys,
            return_ty: BackendTy::Dynamic(DynReason::NotYetSupported),
            locals,
            body: stmts,
            has_this: false,
            this_class: None,
            is_async,
            is_generator,
            has_rest: params.last().is_some_and(|p| p.is_rest),
        };

        let upvalues: Vec<varn_tir::TirUpvalue> = captures
            .iter()
            .map(|name| match self.resolve_name(name) {
                Resolution::Local(id) => varn_tir::TirUpvalue::ParentLocal(id.0),
                Resolution::Param(i) => varn_tir::TirUpvalue::ParentParam(i),
                Resolution::Upvalue(i) => varn_tir::TirUpvalue::ParentUpvalue(i),

                _ => varn_tir::TirUpvalue::ParentUpvalue(0),
            })
            .collect();

        TirExpr {
            kind: TirExprKind::Closure {
                func: func_id,
                upvalues,
            },
            ty,
            res: Resolution::None,
            span,
        }
    }
}
