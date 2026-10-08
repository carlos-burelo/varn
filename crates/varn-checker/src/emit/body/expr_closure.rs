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
        source: Option<ExprId>,
        span: Span,
    ) -> TirExpr {
        let func_id = varn_tir::FnId(self.closure_base + self.out_closures.len() as u32);
        let (sig, param_tys, return_ty) =
            self.closure_signature(source, params.len(), is_async || is_generator);

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
            force_inline: false,
        });
        let slot = self.out_closures.len() - 1;

        let param_names: Vec<Arc<str>> = params
            .iter()
            .map(|p| pattern_lead(&p.pattern, self.m.interner))
            .collect();
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
            sig,
            params: param_tys,
            return_ty,
            locals,
            body: stmts,
            has_this: false,
            this_class: None,
            is_async,
            is_generator,
            has_rest: params.last().is_some_and(|p| p.is_rest),
            force_inline: false,
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

impl FnEmitter<'_> {
    fn closure_signature(
        &mut self,
        source: Option<ExprId>,
        arity: usize,
        coroutine: bool,
    ) -> (SigId, Vec<BackendTy>, BackendTy) {
        let dynamic = BackendTy::Dynamic(DynReason::NotYetSupported);
        let fn_ty = source
            .and_then(|e| self.expr_table.get(&e.index()))
            .map(|entry| entry.ty)
            .filter(|t| matches!(self.m.checker_table.get(t.0), varn_core::TypeKind::Fn(_)));
        let sig = fn_ty.map(|t| {
            crate::emit::tables::intern_signature(
                &t,
                self.m.checker_table,
                self.m.interner,
                self.tt,
                self.m.names,
                self.signatures,
            )
        });
        let checked = sig.and_then(|sig| {
            self.signatures
                .get(sig.0 as usize)
                .filter(|s| s.params.len() == arity)
                .map(|s| (sig, s))
        });
        match checked {
            Some((sig, s)) => {
                let ret = if coroutine { dynamic } else { s.return_ty };
                (sig, s.params.clone(), ret)
            }
            None => (SigId(0), vec![dynamic; arity], dynamic),
        }
    }
}
