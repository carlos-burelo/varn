use super::context::FnEmitter;
use std::sync::Arc;
use varn_core::ast::ExprId;
use varn_tir::{BackendTy, DynReason, Resolution, SigId, Span, TirArg, TirExpr, TirExprKind};

const CONSTRUCTOR: &str = varn_core::TokenKind::Constructor.as_str();

pub(super) enum Callee {
    Value(TirExpr),
    Method { recv: TirExpr, name: Arc<str> },
    Inherited { class: TirExpr, name: Arc<str> },
}

impl Callee {
    pub(super) fn into_expr(self) -> TirExpr {
        match self {
            Callee::Value(value) => value,
            Callee::Method { recv, .. } => recv,
            Callee::Inherited { class, .. } => class,
        }
    }
}

impl<'a> FnEmitter<'a> {
    pub(super) fn callee_signature(&mut self, callee: ExprId) -> Option<SigId> {
        self.expr_table
            .get(&callee.index())
            .map(|entry| entry.ty)
            .filter(|t| matches!(self.m.checker_table.get(t.0), varn_core::TypeKind::Fn(_)))
            .map(|t| {
                crate::tables::intern_signature(
                    &t,
                    self.m.checker_table,
                    self.m.interner,
                    self.tt,
                    self.m.names,
                    self.signatures,
                )
            })
    }

    pub(super) fn settle_super_args(
        &mut self,
        call_id: varn_core::ast::AstId,
        method: &str,
        args: Vec<TirArg>,
        span: Span,
    ) -> Vec<TirArg> {
        let parent = self
            .this_class
            .and_then(|class| self.m.classes.get(class.0 as usize))
            .and_then(|info| info.parent)
            .and_then(|parent| Some((parent, self.m.classes.get(parent.0 as usize)?)));
        let Some((parent, info)) = parent else {
            return args;
        };
        let sig = if method == CONSTRUCTOR {
            crate::tables::inherited_constructor(self.m.classes, parent)
        } else {
            info.method_slot(method)
                .and_then(|slot| info.method_at(slot))
                .map(|entry| entry.sig)
        };
        let name: Arc<str> = Arc::from(method);
        let class = TirExpr {
            kind: TirExprKind::Var,
            ty: BackendTy::Dynamic(DynReason::Runtime),
            res: self.resolve_name(&info.name),
            span,
        };
        let args = self.fill_omitted(args, Some(call_id), sig);
        self.settle_args(
            Some(call_id),
            Callee::Inherited { class, name },
            args,
            sig,
            span,
        )
        .1
    }

    pub(super) fn settle_args(
        &mut self,
        call_id: Option<varn_core::ast::AstId>,
        callee: Callee,
        args: Vec<TirArg>,
        fallback: Option<SigId>,
        span: Span,
    ) -> (Callee, Vec<TirArg>) {
        if !self.unproven(call_id, &callee, &args, fallback) {
            let args = self.guard_args(call_id, args, fallback);
            return (callee, args);
        }
        match callee {
            Callee::Value(c) => {
                let c = self.pin(c);
                let args = self.verify_at_runtime(
                    vec![c.clone()],
                    args,
                    varn_core::runtime_ops::VERIFY_CALL,
                    varn_core::runtime_ops::VERIFY_SPREAD,
                    span,
                );
                (Callee::Value(c), args)
            }
            Callee::Method { recv, name } => {
                let recv = self.pin(recv);
                let label = TirExpr {
                    kind: TirExprKind::StrLit(name.clone()),
                    ty: BackendTy::Str,
                    res: Resolution::None,
                    span,
                };
                let args = self.verify_at_runtime(
                    vec![recv.clone(), label],
                    args,
                    varn_core::runtime_ops::VERIFY_METHOD,
                    varn_core::runtime_ops::VERIFY_METHOD_SPREAD,
                    span,
                );
                (Callee::Method { recv, name }, args)
            }
            Callee::Inherited { class, name } => {
                let class = self.pin(class);
                let label = TirExpr {
                    kind: TirExprKind::StrLit(name.clone()),
                    ty: BackendTy::Str,
                    res: Resolution::None,
                    span,
                };
                let args = self.verify_at_runtime(
                    vec![class.clone(), label],
                    args,
                    varn_core::runtime_ops::VERIFY_SUPER,
                    varn_core::runtime_ops::VERIFY_SUPER_SPREAD,
                    span,
                );
                (Callee::Inherited { class, name }, args)
            }
        }
    }

    pub(super) fn inherited_member(
        &mut self,
        name: &str,
        accessor: bool,
        span: Span,
    ) -> Option<TirExpr> {
        let home = self.home_class?;
        let home_name = self.m.classes.get(home.0 as usize)?.name.clone();
        let opaque = BackendTy::Dynamic(DynReason::Runtime);
        let home = TirExpr {
            kind: TirExprKind::Var,
            ty: opaque,
            res: self.resolve_name(&home_name),
            span,
        };
        let literal = |kind: TirExprKind, ty: BackendTy| TirExpr {
            kind,
            ty,
            res: Resolution::None,
            span,
        };
        let this = self.this_var(span);
        let name = literal(TirExprKind::StrLit(Arc::from(name)), BackendTy::Str);
        let accessor = literal(TirExprKind::BoolLit(accessor), BackendTy::Bool);
        self.call_runtime(
            varn_core::runtime_ops::INHERITED,
            vec![home, this, name, accessor],
            BackendTy::Dynamic(DynReason::Callable),
            span,
        )
    }

    pub(super) fn fill_omitted(
        &mut self,
        args: Vec<TirArg>,
        call_id: Option<varn_core::ast::AstId>,
        fallback: Option<SigId>,
    ) -> Vec<TirArg> {
        let Some((params, has_rest)) = self.shape_of(call_id, fallback) else {
            return args;
        };
        if has_rest || !args.iter().all(|a| matches!(a, TirArg::Expr(_))) {
            return args;
        }
        let mut args = args;
        for param in params.iter().skip(args.len()) {
            let ty = match param {
                BackendTy::Nullable(_) | BackendTy::Dynamic(_) => *param,
                BackendTy::Int
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
                | BackendTy::Never => BackendTy::Dynamic(DynReason::Unrecorded),
            };
            args.push(TirArg::Expr(TirExpr {
                kind: TirExprKind::NullLit,
                ty,
                res: Resolution::None,
                span: Span::EMPTY,
            }));
        }
        args
    }

    pub(super) fn guard_args(
        &mut self,
        call_id: Option<varn_core::ast::AstId>,
        args: Vec<TirArg>,
        fallback: Option<SigId>,
    ) -> Vec<TirArg> {
        let Some((params, has_rest)) = self.shape_of(call_id, fallback) else {
            return args;
        };
        let (fixed, rest): (Vec<BackendTy>, Option<BackendTy>) = match params.split_last() {
            Some((BackendTy::Array(el), head)) if has_rest && self.tt.contains(*el) => {
                (head.to_vec(), Some(self.tt.get(*el)))
            }
            Some(_) | None => (params, None),
        };
        let mut spread_seen = false;
        args.into_iter()
            .enumerate()
            .map(|(i, arg)| match arg {
                TirArg::Expr(e) if !spread_seen => match fixed.get(i).copied().or(rest) {
                    Some(ty) => {
                        let guarded = self.guard_dynamic(e, ty);
                        TirArg::Expr(self.as_declared(guarded, ty))
                    }
                    None => TirArg::Expr(e),
                },
                TirArg::Spread(e) => {
                    spread_seen = true;
                    TirArg::Spread(e)
                }
                after @ TirArg::Expr(_) => after,
                named @ TirArg::Named { .. } => named,
            })
            .collect()
    }
}
