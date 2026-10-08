use super::context::FnEmitter;
use super::small_utils::{bin_op, bool_lit};
use crate::emit::ty::NameResolver;
use varn_core::ast::operators::{BinaryOp, LogicalOp};
use varn_core::ast::{ExprId, ExprKind};
use varn_tir::{BackendTy, DynReason, Resolution, Span, TirBinOp, TirExpr, TirExprKind, TirUnOp};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_binary(
        &mut self,
        op: BinaryOp,
        left: ExprId,
        right: ExprId,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let lhs = self.lower_expr(left);
        let rhs = self.lower_expr(right);

        if op == BinaryOp::Eq || op == BinaryOp::NotEq {
            let is_null_expr = |e: &TirExpr| -> bool {
                matches!(e.kind, TirExprKind::NullLit)
                    || e.ty.non_nullable(self.tt) == BackendTy::Never
            };
            let l_null = is_null_expr(&lhs);
            let r_null = is_null_expr(&rhs);
            if l_null || r_null {
                let is_eq = op == BinaryOp::Eq;
                if l_null && r_null {
                    return TirExpr {
                        kind: TirExprKind::BoolLit(is_eq),
                        ty: BackendTy::Bool,
                        res: Resolution::None,
                        span,
                    };
                }
                let target = if r_null { lhs } else { rhs };
                let is_null = TirExpr {
                    kind: TirExprKind::Unary {
                        op: TirUnOp::IsNull,
                        operand: Box::new(target),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span,
                };
                if is_eq {
                    return is_null;
                }
                return TirExpr {
                    kind: TirExprKind::Unary {
                        op: TirUnOp::Not,
                        operand: Box::new(is_null),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span,
                };
            }
        }

        let Some(top) = bin_op(op) else {
            if op == BinaryOp::Instanceof {
                if let ExprKind::Identifier { name } = &self.ast_arena.expr(right).kind {
                    if let Some(class) = self.m.names.class_id(self.m.interner.resolve(*name)) {
                        return TirExpr {
                            kind: TirExprKind::TypeTest {
                                value: Box::new(lhs),
                                class,
                            },
                            ty: BackendTy::Bool,
                            res: Resolution::None,
                            span,
                        };
                    }

                    return TirExpr {
                        kind: TirExprKind::Binary {
                            op: TirBinOp::Instanceof,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        },
                        ty: BackendTy::Bool,
                        res: Resolution::None,
                        span,
                    };
                }
            }
            if op == BinaryOp::In {
                return TirExpr {
                    kind: TirExprKind::Binary {
                        op: TirBinOp::In,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span,
                };
            }
            return self.cast_to(lhs, BackendTy::Bool);
        };

        let (lhs, rhs, node_ty) = self.coerce_binary_operands(top, lhs, rhs, ty);
        TirExpr {
            kind: TirExprKind::Binary {
                op: top,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
            ty: node_ty,
            res: Resolution::None,
            span,
        }
    }

    pub(super) fn coerce_binary_operands(
        &self,
        op: TirBinOp,
        lhs: TirExpr,
        rhs: TirExpr,
        checked: BackendTy,
    ) -> (TirExpr, TirExpr, BackendTy) {
        let is_cmp = matches!(
            op,
            TirBinOp::Eq | TirBinOp::Ne | TirBinOp::Lt | TirBinOp::Le | TirBinOp::Gt | TirBinOp::Ge
        );
        let l = lhs.ty.non_nullable(self.tt);
        let r = rhs.ty.non_nullable(self.tt);

        if matches!(l, BackendTy::Dynamic(_)) || matches!(r, BackendTy::Dynamic(_)) {
            let ty = if is_cmp {
                BackendTy::Bool
            } else {
                BackendTy::Dynamic(DynReason::NotYetSupported)
            };
            return (lhs, rhs, ty);
        }
        if is_cmp && (l == BackendTy::Never || r == BackendTy::Never) {
            return (lhs, rhs, BackendTy::Bool);
        }

        if l == r {
            let ty = if is_cmp { BackendTy::Bool } else { l };
            return (lhs, rhs, ty);
        }

        let common = if op == TirBinOp::Add
            && (matches!(l, BackendTy::Str) || matches!(r, BackendTy::Str))
        {
            BackendTy::Str
        } else if l == BackendTy::Float || r == BackendTy::Float {
            BackendTy::Float
        } else if matches!(l, BackendTy::Str) || matches!(r, BackendTy::Str) {
            BackendTy::Str
        } else {
            l
        };
        let lhs = self.cast_to(lhs, common);
        let rhs = self.cast_to(rhs, common);
        let ty = if is_cmp { BackendTy::Bool } else { common };
        let _ = checked;
        (lhs, rhs, ty)
    }

    pub(super) fn lower_logical(
        &mut self,
        op: LogicalOp,
        left: ExprId,
        right: ExprId,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let (cond, then_val, else_val) = match op {
            LogicalOp::And | LogicalOp::Or => {
                let l = self.lower_expr(left);
                let l = self.cast_to(l, BackendTy::Bool);
                let r = self.lower_expr(right);
                let r = self.cast_to(r, BackendTy::Bool);
                match op {
                    LogicalOp::And => (l, r, bool_lit(false)),
                    LogicalOp::Or | LogicalOp::Nullish => (l, bool_lit(true), r),
                }
            }
            LogicalOp::Nullish => {
                let l = self.lower_expr(left);
                let l = self.pin(l);
                let r = self.lower_expr(right);
                let is_null = TirExpr {
                    kind: TirExprKind::Unary {
                        op: TirUnOp::IsNull,
                        operand: Box::new(l.clone()),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span,
                };
                (is_null, r, l)
            }
        };

        let (then_val, else_val) =
            if then_val.ty == else_val.ty || matches!(ty, BackendTy::Dynamic(_)) {
                (then_val, else_val)
            } else {
                (self.cast_to(then_val, ty), self.cast_to(else_val, ty))
            };
        TirExpr {
            kind: TirExprKind::Select {
                cond: Box::new(cond),
                then_val: Box::new(then_val),
                else_val: Box::new(else_val),
            },
            ty,
            res: Resolution::None,
            span,
        }
    }

    pub(super) fn lower_unary(
        &mut self,
        op: varn_core::ast::operators::UnaryOp,
        operand: ExprId,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let top = match op {
            varn_core::ast::operators::UnaryOp::Minus => TirUnOp::Neg,
            varn_core::ast::operators::UnaryOp::Not => TirUnOp::Not,
            varn_core::ast::operators::UnaryOp::BitNot => TirUnOp::BitNot,
            varn_core::ast::operators::UnaryOp::Plus => return self.lower_expr(operand),

            varn_core::ast::operators::UnaryOp::Typeof => {
                let inner = self.lower_expr(operand);
                return TirExpr {
                    kind: TirExprKind::Unary {
                        op: TirUnOp::Typeof,
                        operand: Box::new(inner),
                    },
                    ty: BackendTy::Str,
                    res: Resolution::None,
                    span,
                };
            }
        };

        let inner = self.lower_expr(operand);
        TirExpr {
            kind: TirExprKind::Unary {
                op: top,
                operand: Box::new(inner),
            },
            ty,
            res: Resolution::None,
            span,
        }
    }
}
