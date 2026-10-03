use super::context::FnEmitter;
use super::small_utils::assign_bin_op;
use varn_core::ast::operators::AssignOp;
use varn_core::ast::ExprId;
use varn_tir::{BackendTy, Resolution, Span, TirArg, TirExpr, TirExprKind, TirUnOp};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_extension_assign(
        &mut self,
        target: ExprId,
        value: ExprId,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let mangled = self.m.desugar.extension_set_members
            [&self.ast_arena.expr(target).range.start.offset]
            .clone();
        let recv = self.lower_expr(match &self.ast_arena.expr(target).kind {
            varn_core::ast::ExprKind::Member { object, .. } => *object,
            _ => unreachable!(),
        });
        let v = self.lower_expr(value);
        TirExpr {
            kind: TirExprKind::ExtensionCall {
                func: mangled,
                recv: Box::new(recv),
                args: vec![TirArg::Expr(v)],
            },
            ty,
            res: Resolution::None,
            span,
        }
    }

    pub(super) fn lower_index_assign(
        &mut self,
        op: AssignOp,
        object: ExprId,
        property: ExprId,
        value: ExprId,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let obj = self.lower_expr(object);
        let index = self.lower_expr(property);
        let v = self.lower_expr(value);
        let node_ty = match obj.ty.non_nullable(self.tt) {
            BackendTy::Array(el) => self.tt.get(el),
            BackendTy::Map(_, val) => self.tt.get(val),
            _ => v.ty,
        };
        let index_target = |object: TirExpr, index: TirExpr| TirExpr {
            kind: TirExprKind::Index {
                object: Box::new(object),
                index: Box::new(index),
            },
            ty: node_ty,
            res: Resolution::None,
            span,
        };
        let plain = matches!(assign_bin_op(op), Ok(None));
        let (obj_w, index_w) = if plain {
            (obj, index)
        } else {
            (self.pin(obj), self.pin(index))
        };
        let rhs = match assign_bin_op(op) {
            Ok(None) => v,
            Ok(Some(bop)) => {
                let read = index_target(obj_w.clone(), index_w.clone());
                let (lhs, rhs, nty) = self.coerce_binary_operands(bop, read, v, node_ty);
                TirExpr {
                    kind: TirExprKind::Binary {
                        op: bop,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    ty: nty,
                    res: Resolution::None,
                    span,
                }
            }
            Err(()) => {
                use varn_core::ast::operators::AssignOp as A;
                let read = index_target(obj_w.clone(), index_w.clone());
                let cond = match op {
                    A::NullishAssign => TirExpr {
                        kind: TirExprKind::Unary {
                            op: TirUnOp::IsNull,
                            operand: Box::new(read.clone()),
                        },
                        ty: BackendTy::Bool,
                        res: Resolution::None,
                        span,
                    },
                    _ => self.cast_to(read.clone(), BackendTy::Bool),
                };
                let (then_val, else_val) = match op {
                    A::OrAssign => (read.clone(), v),
                    _ => (v, read.clone()),
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
        };
        TirExpr {
            kind: TirExprKind::Assign {
                target: Box::new(index_target(obj_w, index_w)),
                value: Box::new(rhs),
            },
            ty,
            res: Resolution::None,
            span,
        }
    }

    pub(super) fn lower_plain_assign(
        &mut self,
        op: AssignOp,
        target: ExprId,
        value: ExprId,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let t = self.lower_expr(target);
        let t = match assign_bin_op(op) {
            Ok(None) => t,
            _ => self.pin_place(t),
        };
        let v = self.lower_expr(value);
        let rhs = match assign_bin_op(op) {
            Ok(None) => v,
            Ok(Some(bop)) => {
                let (lhs, rhs, nty) = self.coerce_binary_operands(bop, t.clone(), v, t.ty);
                TirExpr {
                    kind: TirExprKind::Binary {
                        op: bop,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    ty: nty,
                    res: Resolution::None,
                    span,
                }
            }

            Err(()) => {
                use varn_core::ast::operators::AssignOp as A;
                let cond = match op {
                    A::NullishAssign => TirExpr {
                        kind: TirExprKind::Unary {
                            op: TirUnOp::IsNull,
                            operand: Box::new(t.clone()),
                        },
                        ty: BackendTy::Bool,
                        res: Resolution::None,
                        span,
                    },
                    _ => self.cast_to(t.clone(), BackendTy::Bool),
                };
                let (then_val, else_val) = match op {
                    A::OrAssign => (t.clone(), v),
                    _ => (v, t.clone()),
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
        };
        TirExpr {
            kind: TirExprKind::Assign {
                target: Box::new(t),
                value: Box::new(rhs),
            },
            ty,
            res: Resolution::None,
            span,
        }
    }
}
