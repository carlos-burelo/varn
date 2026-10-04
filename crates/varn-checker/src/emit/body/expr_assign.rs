use super::context::FnEmitter;
use super::small_utils::assign_bin_op;
use std::sync::Arc;
use varn_core::ast::operators::AssignOp;
use varn_core::ast::ExprId;
use varn_tir::{
    BackendTy, DynReason, Resolution, Span, TirArg, TirExpr, TirExprKind, TirStmt, TirUnOp,
};

impl<'a> FnEmitter<'a> {
    pub(in crate::emit) fn this_field_assign(
        &mut self,
        field: Arc<str>,
        value: TirExpr,
    ) -> TirStmt {
        let this = self.this_var(Span::EMPTY);
        let target = self.field_access(
            this,
            field,
            BackendTy::Dynamic(DynReason::NotYetSupported),
            Span::EMPTY,
        );
        TirStmt::Expr(TirExpr {
            kind: TirExprKind::Assign {
                target: Box::new(target),
                value: Box::new(value),
            },
            ty: BackendTy::Void,
            res: Resolution::None,
            span: Span::EMPTY,
        })
    }

    pub(in crate::emit) fn this_param_field_assign(
        &mut self,
        field: Arc<str>,
        param: u32,
        ty: BackendTy,
    ) -> TirStmt {
        let value = TirExpr {
            kind: TirExprKind::Var,
            ty,
            res: Resolution::Param(param),
            span: Span::EMPTY,
        };
        self.this_field_assign(field, value)
    }

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
                let read = index_target(obj_w.clone(), index_w.clone());
                self.lower_logical_assign(op, read, v, span)
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

            Err(()) => self.lower_logical_assign(op, t.clone(), v, span),
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

    fn lower_logical_assign(
        &mut self,
        op: AssignOp,
        read: TirExpr,
        v: TirExpr,
        span: Span,
    ) -> TirExpr {
        let place_ty = read.ty;
        let cond = match op {
            AssignOp::NullishAssign => TirExpr {
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
        let v = self.cast_to(v, place_ty);
        let (then_val, else_val) = match op {
            AssignOp::OrAssign => (read, v),
            _ => (v, read),
        };
        TirExpr {
            kind: TirExprKind::Select {
                cond: Box::new(cond),
                then_val: Box::new(then_val),
                else_val: Box::new(else_val),
            },
            ty: place_ty,
            res: Resolution::None,
            span,
        }
    }
}
