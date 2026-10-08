use super::context::FnEmitter;
use super::small_utils::int_lit;
use varn_core::ast::operators::UpdateOp;
use varn_core::ast::ExprId;
use varn_tir::{BackendTy, Resolution, Span, TirBinOp, TirExpr, TirExprKind, TirStmt};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_index_update(
        &mut self,
        op: UpdateOp,
        object: ExprId,
        property: ExprId,
        prefix: bool,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let obj = self.lower_expr(object);
        let index = self.lower_expr(property);
        let node_ty = match obj.ty.non_nullable(self.tt) {
            BackendTy::Array(el) => self.tt.get(el),
            BackendTy::Map(_, val) => self.tt.get(val),
            BackendTy::Int | BackendTy::Float | BackendTy::Bool | BackendTy::Char | BackendTy::Str | BackendTy::Bytes | BackendTy::Decimal | BackendTy::BigInt | BackendTy::Set(_) | BackendTy::Tuple(_) | BackendTy::Class(_) | BackendTy::Enum(_) | BackendTy::Fn(_) | BackendTy::Nullable(_) | BackendTy::Void | BackendTy::Never | BackendTy::Dynamic(_) => ty,
        };
        let obj_h = self.pin(obj);
        let index_h = self.pin(index);
        let read = TirExpr {
            kind: TirExprKind::Index {
                object: Box::new(obj_h.clone()),
                index: Box::new(index_h.clone()),
            },
            ty: node_ty,
            res: Resolution::None,
            span,
        };
        let bop = match op {
            UpdateOp::Increment => TirBinOp::Add,
            UpdateOp::Decrement => TirBinOp::Sub,
        };
        let step = if node_ty == BackendTy::Float {
            self.cast_to(int_lit(1), BackendTy::Float)
        } else {
            int_lit(1)
        };
        let old = if prefix {
            read.clone()
        } else {
            self.hoist(read.clone())
        };
        let (lhs, rhs, nty) = self.coerce_binary_operands(bop, old.clone(), step, node_ty);
        let stepped = TirExpr {
            kind: TirExprKind::Binary {
                op: bop,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
            ty: nty,
            res: Resolution::None,
            span,
        };
        let assign = TirExpr {
            kind: TirExprKind::Assign {
                target: Box::new(TirExpr {
                    kind: TirExprKind::Index {
                        object: Box::new(obj_h),
                        index: Box::new(index_h),
                    },
                    ty: node_ty,
                    res: Resolution::None,
                    span,
                }),
                value: Box::new(stepped),
            },
            ty: nty,
            res: Resolution::None,
            span,
        };
        if prefix {
            return assign;
        }
        self.pending.push(TirStmt::Expr(assign));
        old
    }

    pub(super) fn lower_plain_update(
        &mut self,
        op: UpdateOp,
        operand: ExprId,
        prefix: bool,
        span: Span,
    ) -> TirExpr {
        let t = self.lower_expr(operand);
        let t = self.pin_place(t);
        let bop = match op {
            UpdateOp::Increment => TirBinOp::Add,
            UpdateOp::Decrement => TirBinOp::Sub,
        };
        let step = if t.ty == BackendTy::Float {
            self.cast_to(int_lit(1), BackendTy::Float)
        } else {
            int_lit(1)
        };

        let old = if prefix {
            t.clone()
        } else {
            self.hoist(t.clone())
        };
        let (lhs, rhs, nty) = self.coerce_binary_operands(bop, old.clone(), step, t.ty);
        let stepped = TirExpr {
            kind: TirExprKind::Binary {
                op: bop,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
            ty: nty,
            res: Resolution::None,
            span,
        };
        let assign = TirExpr {
            kind: TirExprKind::Assign {
                target: Box::new(t),
                value: Box::new(stepped),
            },
            ty: nty,
            res: Resolution::None,
            span,
        };
        if prefix {
            return assign;
        }

        self.pending.push(TirStmt::Expr(assign));
        old
    }
}
