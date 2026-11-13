use super::call_contract::Callee;
use super::context::FnEmitter;
use crate::NameResolver;
use std::sync::Arc;
use vn_core::ast::{Arg, AstId, ExprId, ExprKind};
use vn_tir::{BackendTy, Resolution, Span, TirArg, TirExpr, TirExprKind};

impl FnEmitter<'_> {
    pub(super) fn namespace_path(&mut self, e: ExprId) -> Option<Arc<str>> {
        if let ExprKind::Identifier { name } = &self.ast_arena.expr(e).kind {
            let bare = self.m.interner.resolve(*name);
            let mut candidates: Vec<Arc<str>> = self
                .scopes_outward()
                .into_iter()
                .map(|scope| crate::namespaces::scoped_name(Some(scope), bare))
                .collect();
            candidates.push(Arc::from(bare));
            let path = candidates
                .into_iter()
                .find(|key| self.m.namespaces.paths.contains(key))?;
            let slot = *self.m.globals.get(&path)?;
            return (self.resolve_name(bare) == Resolution::GlobalSlot(slot)).then_some(path);
        }
        let ExprKind::Member {
            object,
            property,
            computed: false,
            optional: false,
        } = &self.ast_arena.expr(e).kind
        else {
            return None;
        };
        let (object, property) = (*object, *property);
        let outer = self.namespace_path(object)?;
        let name = Self::member_name(self.ast_arena, property, self.m.interner)?;
        let path = crate::namespaces::scoped_name(Some(&outer), &name);
        self.m.namespaces.paths.contains(&path).then_some(path)
    }

    pub(super) fn namespace_member(
        &mut self,
        object: ExprId,
        property: ExprId,
    ) -> Option<Arc<str>> {
        let path = self.namespace_path(object)?;
        let name = Self::member_name(self.ast_arena, property, self.m.interner)?;
        let key = crate::namespaces::scoped_name(Some(&path), &name);
        self.m.globals.contains_key(&key).then_some(key)
    }

    pub(super) fn class_named(&self, written: &str) -> Option<vn_tir::ClassId> {
        crate::namespace_scope::scoped_lookup(self.namespace.as_deref(), written, |key| {
            self.m.names.class_id(key)
        })
    }

    pub(super) fn typed_callee(&self, mut callee: TirExpr, sig: Option<vn_tir::SigId>) -> TirExpr {
        if let Some(sig) = sig
            && matches!(callee.ty, BackendTy::Dynamic(vn_tir::DynReason::Callable))
        {
            callee.ty = BackendTy::Fn(sig);
        }
        callee
    }

    pub(super) fn forward_keeps_checks(
        &self,
        targs: &[TirArg],
        callee_sig: Option<vn_tir::SigId>,
    ) -> bool {
        let params = callee_sig
            .and_then(|sig| self.signatures.get(sig.0 as usize))
            .map(|sig| sig.params.as_slice());
        targs.iter().enumerate().all(|(slot, arg)| {
            let untyped = matches!(arg.value().ty, BackendTy::Dynamic(_));
            let accepts_any = params
                .and_then(|params| params.get(slot))
                .is_some_and(|param| matches!(param, BackendTy::Dynamic(_)));
            !untyped || accepts_any
        })
    }

    pub(super) fn lower_namespace_call(
        &mut self,
        call_id: AstId,
        callee: ExprId,
        args: &[Arg],
        ty: BackendTy,
        span: Span,
    ) -> Option<TirExpr> {
        let ExprKind::Member {
            object,
            property,
            computed: false,
            optional: false,
        } = &self.ast_arena.expr(callee).kind
        else {
            return None;
        };
        let (object, property) = (*object, *property);
        let key = self.namespace_member(object, property)?;
        let c = self.lower_expr(callee);
        let targs = self.lower_call_args(call_id, args);
        let callee_sig = self.callee_signature(callee);
        let (target, targs) =
            self.settle_args(Some(call_id), Callee::Value(c), targs, callee_sig, span);
        let c = target.into_expr();
        let all_positional = targs.iter().all(|a| matches!(a, TirArg::Expr(_)));
        let takes_rest = callee_sig
            .and_then(|sig| self.signatures.get(sig.0 as usize))
            .is_none_or(|sig| sig.has_rest);
        let known = self.m.scoped_fns.get(&key).copied();
        let mut targs = targs;
        if let Some((_, arity)) = known.filter(|_| all_positional && !takes_rest) {
            targs = self.fill_omitted(targs, Some(call_id), callee_sig);
            while targs.len() < arity as usize {
                targs.push(TirArg::Expr(TirExpr {
                    kind: TirExprKind::NullLit,
                    ty: crate::ty::NULL_TY,
                    res: Resolution::None,
                    span: Span::EMPTY,
                }));
            }
        }
        let forwarded = self
            .m
            .native_forwards
            .get(&key)
            .filter(|(_, args, _)| {
                all_positional
                    && !targs.is_empty()
                    && targs.len() <= *args as usize
                    && self.forward_keeps_checks(&targs, callee_sig)
            });
        let res = match (known, forwarded) {
            (_, Some((plain, _, Some(typed)))) => Resolution::NativeDecode {
                plain: *plain,
                typed: *typed,
            },
            (_, Some((plain, _, None))) => Resolution::NativeOp(*plain),
            (Some((fn_id, arity)), None) if all_positional && arity as usize == targs.len() => {
                Resolution::DirectFn(vn_tir::FnId(fn_id))
            }
            (Some(_) | None, None) => Resolution::None,
        };
        let c = self.typed_callee(c, callee_sig);
        Some(TirExpr {
            kind: TirExprKind::Call {
                callee: Box::new(c),
                args: targs,
            },
            ty,
            res,
            span,
        })
    }
}
