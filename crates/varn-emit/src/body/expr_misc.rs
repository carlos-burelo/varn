use super::context::FnEmitter;
use varn_core::ast::ExprId;
use varn_tir::{BackendTy, TirExpr, TirExprKind};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_cond(&mut self, e: ExprId) -> TirExpr {
        let lowered = self.lower_expr(e);
        match lowered.ty {
            BackendTy::Bool | BackendTy::Dynamic(_) => lowered,
            BackendTy::Int
            | BackendTy::Float
            | BackendTy::Char
            | BackendTy::Str
            | BackendTy::Bytes
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Class(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Nullable(_)
            | BackendTy::Void
            | BackendTy::Never => self.cast_to(lowered, BackendTy::Bool),
        }
    }

    pub(super) fn to_str(&self, e: TirExpr) -> TirExpr {
        if e.ty == BackendTy::Str {
            return e;
        }
        let span = e.span;
        TirExpr {
            kind: TirExprKind::Unary {
                op: varn_tir::TirUnOp::ToStr,
                operand: Box::new(e),
            },
            ty: BackendTy::Str,
            res: varn_tir::Resolution::None,
            span,
        }
    }

    pub(super) fn assert_type(&self, e: TirExpr, ty: BackendTy) -> TirExpr {
        let numeric = matches!(ty.non_nullable(self.tt), BackendTy::Int | BackendTy::Float);
        if !numeric && self.crosses_dynamic_boundary(&e, ty) {
            self.guard_dynamic(e, ty)
        } else {
            self.cast_to(e, ty)
        }
    }

    fn crosses_dynamic_boundary(&self, e: &TirExpr, ty: BackendTy) -> bool {
        let BackendTy::Dynamic(reason) = e.ty else {
            return false;
        };
        let untyped = untyped_reason(reason);
        match ty.non_nullable(self.tt) {
            BackendTy::Str
            | BackendTy::Bool
            | BackendTy::Int
            | BackendTy::Float
            | BackendTy::Class(_) => true,
            BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Char
            | BackendTy::Bytes
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Dynamic(varn_tir::DynReason::Callable) => untyped,
            BackendTy::Dynamic(
                varn_tir::DynReason::HostBoundary
                | varn_tir::DynReason::Union
                | varn_tir::DynReason::IndexSignature
                | varn_tir::DynReason::Declared
                | varn_tir::DynReason::Awaitable(_)
                | varn_tir::DynReason::Yields(_)
                | varn_tir::DynReason::NotYetSupported,
            )
            | BackendTy::Nullable(_)
            | BackendTy::Void
            | BackendTy::Never => false,
        }
    }

    fn is_untyped(&self, ty: BackendTy) -> bool {
        match ty {
            BackendTy::Dynamic(reason) => untyped_reason(reason),
            BackendTy::Array(el) => {
                self.tt.contains(el)
                    && matches!(self.tt.get(el), BackendTy::Dynamic(reason) if untyped_reason(reason))
            }
            BackendTy::Int
            | BackendTy::Float
            | BackendTy::Bool
            | BackendTy::Char
            | BackendTy::Str
            | BackendTy::Bytes
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Map(..)
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

    pub(super) fn guard_args(
        &mut self,
        args: Vec<varn_tir::TirArg>,
        sig: Option<varn_tir::SigId>,
    ) -> Vec<varn_tir::TirArg> {
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
        let mut out = Vec::with_capacity(args.len());
        for (i, arg) in args.into_iter().enumerate() {
            if spread_seen {
                out.push(arg);
                continue;
            }
            out.push(match arg {
                varn_tir::TirArg::Expr(e) => match fixed.get(i).copied().or(rest) {
                    Some(ty) => varn_tir::TirArg::Expr(self.guard_dynamic(e, ty)),
                    None => varn_tir::TirArg::Expr(e),
                },
                varn_tir::TirArg::Spread(e) => {
                    spread_seen = true;
                    if !self.is_untyped(e.ty) {
                        varn_tir::TirArg::Spread(e)
                    } else {
                        let shape = varn_tir::SpreadShape {
                            fixed: self.tt.intern_list(fixed.get(i..).unwrap_or(&[])),
                            rest: rest.map(|ty| self.tt.intern(ty)),
                        };
                        let (ty, span) = (e.ty, e.span);
                        varn_tir::TirArg::Spread(TirExpr {
                            kind: TirExprKind::Checked {
                                operand: Box::new(e),
                                spread: Some(shape),
                            },
                            ty,
                            res: varn_tir::Resolution::None,
                            span,
                        })
                    }
                }
                named @ varn_tir::TirArg::Named { .. } => named,
            });
        }
        out
    }

    pub(super) fn guard_dynamic(&self, e: TirExpr, ty: BackendTy) -> TirExpr {
        if !self.crosses_dynamic_boundary(&e, ty) {
            return e;
        }
        let span = e.span;
        TirExpr {
            kind: TirExprKind::Checked {
                operand: Box::new(e),
                spread: None,
            },
            ty,
            res: varn_tir::Resolution::None,
            span,
        }
    }

    pub(super) fn cast_to(&self, e: TirExpr, ty: BackendTy) -> TirExpr {
        if e.ty == ty {
            return e;
        }
        let span = e.span;
        TirExpr {
            kind: TirExprKind::Cast {
                operand: Box::new(e),
            },
            ty,
            res: varn_tir::Resolution::None,
            span,
        }
    }
}

fn untyped_reason(reason: varn_tir::DynReason) -> bool {
    match reason {
        varn_tir::DynReason::Declared | varn_tir::DynReason::HostBoundary => true,
        varn_tir::DynReason::Union
        | varn_tir::DynReason::IndexSignature
        | varn_tir::DynReason::Callable
        | varn_tir::DynReason::Awaitable(_)
        | varn_tir::DynReason::Yields(_)
        | varn_tir::DynReason::NotYetSupported => false,
    }
}
