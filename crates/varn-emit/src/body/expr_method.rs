use super::call_contract::Callee;
use super::context::FnEmitter;
use super::small_utils::{bin_op, int_lit};
use std::sync::Arc;
use varn_core::ast::ExprId;
use varn_tir::{BackendTy, DynReason, Resolution, Span, TirArg, TirExpr, TirExprKind, TirUnOp};

impl<'a> FnEmitter<'a> {
    pub(super) fn member_call(
        &mut self,
        recv: TirExpr,
        name: Arc<str>,
        targs: Vec<TirArg>,
        ty: BackendTy,
        checked: Option<varn_tir::SigId>,
        span: Span,
    ) -> TirExpr {
        let all_positional = targs.iter().all(|a| matches!(a, TirArg::Expr(_)));

        if all_positional {
            if let Some(cls) = self.core_class_name(recv.ty) {
                if self
                    .m
                    .core_ops
                    .contains(&(Arc::from(cls), Arc::clone(&name)))
                {
                    let op_id = varn_core::op_id::core_method_op_id(cls, &name);
                    return TirExpr {
                        kind: TirExprKind::MethodCall {
                            recv: Box::new(recv),
                            name,
                            args: targs,
                        },
                        ty,
                        res: Resolution::NativeOp(op_id),
                        span,
                    };
                }
            }
        }

        self.method_call(recv, name, targs, ty, checked, span)
    }

    pub(super) fn method_call(
        &mut self,
        recv: TirExpr,
        name: Arc<str>,
        targs: Vec<TirArg>,
        ty: BackendTy,
        checked: Option<varn_tir::SigId>,
        span: Span,
    ) -> TirExpr {
        let class = self.class_of(recv.ty);
        let table_sig = class.and_then(|ci| {
            let slot = ci.method_slot(&name)?;
            Some(ci.method_at(slot)?.sig)
        });
        let method_sig = checked.filter(|_| class.is_some()).or(table_sig);
        let (recv, targs) = if self.core_class_name(recv.ty).is_some() {
            (recv, self.guard_args(None, targs, method_sig))
        } else {
            let targs = self.fill_omitted(targs, None, method_sig);
            let target = Callee::Method {
                recv,
                name: name.clone(),
            };
            let (target, targs) = self.settle_args(None, target, targs, method_sig, span);
            (target.into_expr(), targs)
        };
        let all_positional = targs.iter().all(|a| matches!(a, TirArg::Expr(_)));
        let res = self
            .class_of(recv.ty)
            .and_then(|ci| ci.method_slot(&name).map(|s| (s, ci)))
            .and_then(|(slot, ci)| {
                let entry = ci.method_at(slot)?;
                let sig = self.signatures.get(entry.sig.0 as usize)?;
                (all_positional && sig.params.len() == targs.len())
                    .then_some(Resolution::VtableSlot(slot))
            })
            .unwrap_or(Resolution::ByName {
                name: name.clone(),
                why: DynReason::NotYetSupported,
            });

        TirExpr {
            kind: TirExprKind::MethodCall {
                recv: Box::new(recv),
                name,
                args: targs,
            },
            ty,
            res,
            span,
        }
    }

    pub(super) fn lower_operator_call(
        &mut self,
        method: varn_core::capability::OperatorMethod,
        recv: ExprId,
        arg: Option<ExprId>,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        use varn_core::capability::OperatorShape;
        let recv = self.lower_expr(recv);
        let args = arg
            .map(|a| vec![TirArg::Expr(self.lower_expr(a))])
            .unwrap_or_default();
        let name: Arc<str> = Arc::from(method.method);
        match method.shape {
            OperatorShape::Value => self.method_call(recv, name, args, ty, None, span),
            OperatorShape::Equals => self.method_call(recv, name, args, BackendTy::Bool, None, span),
            OperatorShape::NotEquals => {
                let equals = self.method_call(recv, name, args, BackendTy::Bool, None, span);
                TirExpr {
                    kind: TirExprKind::Unary {
                        op: TirUnOp::Not,
                        operand: Box::new(equals),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span,
                }
            }
            OperatorShape::CompareToZero(op) => {
                let compared = self.method_call(recv, name, args, BackendTy::Int, None, span);
                TirExpr {
                    kind: TirExprKind::Binary {
                        op: bin_op(op).expect("comparison operators lower to a TirBinOp"),
                        lhs: Box::new(compared),
                        rhs: Box::new(int_lit(0)),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span,
                }
            }
        }
    }
}
