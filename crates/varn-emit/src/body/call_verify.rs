use super::call_contract::Callee;
use super::context::FnEmitter;
use super::expr_misc::untyped_reason;
use std::sync::Arc;
use varn_tir::{
    BackendTy, DynReason, Resolution, SigId, Span, TirArg, TirArrayEl, TirExpr, TirExprKind,
    TirStmt,
};

impl<'a> FnEmitter<'a> {
    pub(super) fn is_untyped(&self, ty: BackendTy) -> bool {
        matches!(ty, BackendTy::Dynamic(reason) if untyped_reason(reason))
    }

    pub(super) fn spreads_untyped(&self, ty: BackendTy) -> bool {
        match ty {
            BackendTy::Dynamic(reason) => untyped_reason(reason),
            BackendTy::Array(el) => self.tt.contains(el) && self.is_untyped(self.tt.get(el)),
            BackendTy::Literal(_)
            | BackendTy::Int
            | BackendTy::Float
            | BackendTy::Bool
            | BackendTy::Char
            | BackendTy::Str
            | BackendTy::Bytes
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Map(..)
            | BackendTy::Dict(_)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Class(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Nullable(_)
            | BackendTy::Void
            | BackendTy::Never => false,
        }
    }

    pub(super) fn unproven(
        &self,
        call_id: Option<varn_core::ast::AstId>,
        callee: &Callee,
        args: &[TirArg],
        fallback: Option<SigId>,
    ) -> bool {
        if args.iter().any(|a| matches!(a, TirArg::Named { .. })) {
            return false;
        }
        let untyped_arg = |arg: &TirArg| match arg {
            TirArg::Expr(e) => self.is_untyped(e.ty),
            TirArg::Spread(e) => self.spreads_untyped(e.ty),
            TirArg::Named { .. } => false,
        };
        if let Some(first_spread) = args.iter().position(TirArg::is_spread)
            && args[first_spread..].iter().any(untyped_arg)
        {
            return true;
        }
        if fallback.is_some() {
            return false;
        }
        if call_id.is_some_and(|id| self.m.desugar.call_params.contains_key(&id)) {
            return false;
        }
        let untyped_callee = match callee {
            Callee::Value(c) => self.is_untyped(c.ty),
            Callee::Method { recv, .. } => self.is_untyped(recv.ty),
            Callee::Inherited { .. } => false,
        };
        untyped_callee || args.iter().any(untyped_arg)
    }

    pub(super) fn verify_at_runtime(
        &mut self,
        mut lead: Vec<TirExpr>,
        args: Vec<TirArg>,
        positional: &str,
        spread: &str,
        span: Span,
    ) -> Vec<TirArg> {
        let opaque = BackendTy::Dynamic(DynReason::Declared);
        if args.iter().any(TirArg::is_spread) {
            let elements = args
                .into_iter()
                .map(|arg| match arg {
                    TirArg::Expr(e) | TirArg::Named { value: e, .. } => TirArrayEl::Expr(e),
                    TirArg::Spread(e) => TirArrayEl::Spread(e),
                })
                .collect();
            let element = self.tt.intern(opaque);
            let flat = self.hoist(TirExpr {
                kind: TirExprKind::ArrayLit(elements),
                ty: BackendTy::Array(element),
                res: Resolution::None,
                span,
            });
            lead.push(flat.clone());
            if let Some(check) =
                self.call_runtime(spread, lead, BackendTy::Dynamic(DynReason::Declared), span)
            {
                self.pending.push(TirStmt::Expr(check));
            }
            return vec![TirArg::Spread(flat)];
        }
        let pinned: Vec<TirExpr> = args
            .into_iter()
            .map(|arg| match arg {
                TirArg::Expr(e) | TirArg::Spread(e) | TirArg::Named { value: e, .. } => self.pin(e),
            })
            .collect();
        lead.extend(pinned.iter().cloned());
        if let Some(check) = self.call_runtime(positional, lead, BackendTy::Void, span) {
            self.pending.push(TirStmt::Expr(check));
        }
        pinned.into_iter().map(TirArg::Expr).collect()
    }

    pub(super) fn call_runtime(
        &mut self,
        op: &str,
        args: Vec<TirExpr>,
        ty: BackendTy,
        span: Span,
    ) -> Option<TirExpr> {
        let mut args = args.into_iter();
        let subject = args.next()?;
        Some(TirExpr {
            kind: TirExprKind::MethodCall {
                recv: Box::new(subject),
                name: Arc::from(op),
                args: args.map(TirArg::Expr).collect(),
            },
            ty,
            res: Resolution::NativeOp(varn_core::runtime_ops::op_id(op)),
            span,
        })
    }

    pub(super) fn as_declared(&self, arg: TirExpr, param: BackendTy) -> TirExpr {
        let boxed_already = matches!(arg.ty, BackendTy::Nullable(_) | BackendTy::Dynamic(_));
        match param {
            BackendTy::Nullable(_) | BackendTy::Dynamic(_) if boxed_already => arg,
            BackendTy::Nullable(_) => self.cast_to(arg, param),
            BackendTy::Dynamic(_) => self.cast_to(arg, param),
            BackendTy::Literal(_)
            | BackendTy::Int
            | BackendTy::Float
            | BackendTy::Bool
            | BackendTy::Char
            | BackendTy::Str
            | BackendTy::Bytes
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Dict(_)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Class(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Void
            | BackendTy::Never => arg,
        }
    }

    pub(super) fn shape_of(
        &mut self,
        call_id: Option<varn_core::ast::AstId>,
        fallback: Option<SigId>,
    ) -> Option<(Vec<BackendTy>, bool)> {
        if let Some(call_id) = call_id
            && let Some(shape) = self.checked_shape(call_id)
        {
            return Some(shape);
        }
        let sig = fallback.and_then(|s| self.signatures.get(s.0 as usize))?;
        Some((sig.params.clone(), sig.has_rest))
    }
}
