use super::context::FnEmitter;
use super::expr_misc::untyped_reason;
use std::sync::Arc;
use varn_core::ast::ExprId;
use varn_tir::{
    BackendTy, DynReason, Resolution, SigId, Span, TirArg, TirArrayEl, TirExpr, TirExprKind,
    TirStmt,
};

const CONSTRUCTOR: &str = "constructor";

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
            ty: BackendTy::Dynamic(DynReason::NotYetSupported),
            res: self.resolve_name(&info.name),
            span,
        };
        let args = self.fill_omitted(args, sig);
        self.settle_args(Callee::Inherited { class, name }, args, sig, span)
            .1
    }

    fn is_untyped(&self, ty: BackendTy) -> bool {
        matches!(ty, BackendTy::Dynamic(reason) if untyped_reason(reason))
    }

    fn spreads_untyped(&self, ty: BackendTy) -> bool {
        match ty {
            BackendTy::Dynamic(reason) => untyped_reason(reason),
            BackendTy::Array(el) => self.tt.contains(el) && self.is_untyped(self.tt.get(el)),
            BackendTy::Int
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

    fn unproven(&self, callee: &Callee, args: &[TirArg], sig: Option<SigId>) -> bool {
        if args.iter().any(|a| matches!(a, TirArg::Named { .. })) {
            return false;
        }
        let untyped_arg = |arg: &TirArg| match arg {
            TirArg::Expr(e) => self.is_untyped(e.ty),
            TirArg::Spread(e) => self.spreads_untyped(e.ty),
            TirArg::Named { .. } => false,
        };
        if let Some(first_spread) = args.iter().position(TirArg::is_spread) {
            if args[first_spread..].iter().any(untyped_arg) {
                return true;
            }
        }
        if sig.is_some() {
            return false;
        }
        let untyped_callee = match callee {
            Callee::Value(c) => self.is_untyped(c.ty),
            Callee::Method { recv, .. } => self.is_untyped(recv.ty),
            Callee::Inherited { .. } => false,
        };
        untyped_callee || args.iter().any(untyped_arg)
    }

    pub(super) fn settle_args(
        &mut self,
        callee: Callee,
        args: Vec<TirArg>,
        sig: Option<SigId>,
        span: Span,
    ) -> (Callee, Vec<TirArg>) {
        if !self.unproven(&callee, &args, sig) {
            let args = self.guard_args(args, sig);
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

    fn verify_at_runtime(
        &mut self,
        mut lead: Vec<TirExpr>,
        args: Vec<TirArg>,
        positional: &str,
        spread: &str,
        span: Span,
    ) -> Vec<TirArg> {
        let opaque = BackendTy::Dynamic(DynReason::NotYetSupported);
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
            if let Some(check) = self.call_runtime(spread, lead, span) {
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
        let Some(converted) = self.call_runtime(positional, lead, span) else {
            return pinned.into_iter().map(TirArg::Expr).collect();
        };
        let converted = self.hoist(converted);
        let unchanged = TirExpr {
            kind: TirExprKind::Unary {
                op: varn_tir::TirUnOp::IsNull,
                operand: Box::new(converted.clone()),
            },
            ty: BackendTy::Bool,
            res: Resolution::None,
            span,
        };
        pinned
            .into_iter()
            .enumerate()
            .map(|(i, arg)| {
                if !matches!(arg.ty, BackendTy::Dynamic(_)) {
                    return TirArg::Expr(arg);
                }
                let ty = arg.ty;
                let replaced = TirExpr {
                    kind: TirExprKind::Index {
                        object: Box::new(converted.clone()),
                        index: Box::new(TirExpr {
                            kind: TirExprKind::IntLit(i as i64),
                            ty: BackendTy::Int,
                            res: Resolution::None,
                            span,
                        }),
                    },
                    ty,
                    res: Resolution::None,
                    span,
                };
                TirArg::Expr(TirExpr {
                    kind: TirExprKind::Select {
                        cond: Box::new(unchanged.clone()),
                        then_val: Box::new(arg),
                        else_val: Box::new(replaced),
                    },
                    ty,
                    res: Resolution::None,
                    span,
                })
            })
            .collect()
    }

    fn call_runtime(&mut self, op: &str, args: Vec<TirExpr>, span: Span) -> Option<TirExpr> {
        let mut args = args.into_iter();
        let subject = args.next()?;
        Some(TirExpr {
            kind: TirExprKind::MethodCall {
                recv: Box::new(subject),
                name: Arc::from(op),
                args: args.map(TirArg::Expr).collect(),
            },
            ty: BackendTy::Dynamic(DynReason::NotYetSupported),
            res: Resolution::NativeOp(varn_core::runtime_ops::op_id(op)),
            span,
        })
    }

    fn as_declared(&self, arg: TirExpr, param: BackendTy) -> TirExpr {
        let boxed_already = matches!(arg.ty, BackendTy::Nullable(_) | BackendTy::Dynamic(_));
        match param {
            BackendTy::Nullable(_) | BackendTy::Dynamic(_) if boxed_already => arg,
            BackendTy::Nullable(inner) => {
                let present = if self.tt.contains(inner)
                    && self.tt.get(inner) == BackendTy::Float
                    && arg.ty == BackendTy::Int
                {
                    self.cast_to(arg, BackendTy::Float)
                } else {
                    arg
                };
                self.cast_to(present, param)
            }
            BackendTy::Dynamic(_) => self.cast_to(arg, param),
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
            | BackendTy::Never => arg,
        }
    }

    pub(super) fn fill_omitted(&self, mut args: Vec<TirArg>, sig: Option<SigId>) -> Vec<TirArg> {
        let Some(sig) = sig.and_then(|s| self.signatures.get(s.0 as usize)) else {
            return args;
        };
        if sig.has_rest || !args.iter().all(|a| matches!(a, TirArg::Expr(_))) {
            return args;
        }
        for param in sig.params.iter().skip(args.len()) {
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
                | BackendTy::Never => BackendTy::Dynamic(DynReason::NotYetSupported),
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

    pub(super) fn guard_args(&mut self, args: Vec<TirArg>, sig: Option<SigId>) -> Vec<TirArg> {
        let Some(sig) = sig.and_then(|s| self.signatures.get(s.0 as usize)) else {
            return args;
        };
        let (fixed, rest): (Vec<BackendTy>, Option<BackendTy>) = match sig.params.split_last() {
            Some((BackendTy::Array(el), head)) if sig.has_rest && self.tt.contains(*el) => {
                (head.to_vec(), Some(self.tt.get(*el)))
            }
            Some(_) | None => (sig.params.clone(), None),
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
