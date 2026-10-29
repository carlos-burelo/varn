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

        if all_positional
            && let Some(cls) = self.core_class_name(recv.ty)
            && let Some(&op_id) = self.m.core_ops.get(&(Arc::from(cls), Arc::clone(&name)))
        {
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

        self.method_call(recv, name, targs, ty, checked, span)
    }

    pub(super) fn method_resolution(
        &self,
        recv_ty: BackendTy,
        name: &Arc<str>,
        arg_count: usize,
        all_positional: bool,
    ) -> Resolution {
        let slotted = match recv_ty.non_nullable(self.tt) {
            BackendTy::Enum(id) => self.m.enums.get(id.0 as usize).and_then(|info| {
                let slot = info.member_slot(name)?;
                Some((slot, info.member_at(slot)?.sig))
            }),
            BackendTy::Class(_) => self.class_of(recv_ty).and_then(|ci| {
                let slot = ci.method_slot(name)?;
                Some((slot, ci.method_at(slot)?.sig))
            }),
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
            | BackendTy::Fn(_)
            | BackendTy::Nullable(_)
            | BackendTy::Void
            | BackendTy::Never
            | BackendTy::Dynamic(_) => None,
        };
        if let Some((slot, sig)) = slotted {
            if !all_positional {
                return Resolution::ByName {
                    name: name.clone(),
                    why: DynReason::Spread,
                };
            }
            if self.signatures.get(sig.0 as usize).is_some_and(|sig| {
                sig.params.len() == arg_count || (sig.has_rest && arg_count + 1 >= sig.params.len())
            }) {
                return Resolution::VtableSlot(slot);
            }
        }
        if let Some(field) = self.class_of(recv_ty).and_then(|ci| ci.field(name)) {
            return Resolution::FieldSlot(field.slot);
        }
        self.by_name(name.clone(), recv_ty)
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
        if let Some(ci) = self.class_of(recv.ty)
            && Self::is_class_ref(&recv, &ci.name, self.m.globals)
            && let Some(slot) = ci.static_slot(&name)
        {
            let sig = checked.or(ci.static_at(slot).and_then(|s| s.sig));
            let declared = ci.static_at(slot).map(|s| s.ty).unwrap_or(ty);
            let callee = TirExpr {
                kind: TirExprKind::Field {
                    object: Box::new(recv),
                    name: name.clone(),
                },
                ty: declared,
                res: Resolution::StaticField(slot),
                span,
            };
            let targs = self.fill_omitted(targs, None, sig);
            let (target, targs) = self.settle_args(None, Callee::Value(callee), targs, sig, span);
            return TirExpr {
                kind: TirExprKind::Call {
                    callee: Box::new(target.into_expr()),
                    args: targs,
                },
                ty,
                res: Resolution::None,
                span,
            };
        }
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
        let res = self.method_resolution(recv.ty, &name, targs.len(), all_positional);

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
            OperatorShape::Equals => {
                self.method_call(recv, name, args, BackendTy::Bool, None, span)
            }
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
