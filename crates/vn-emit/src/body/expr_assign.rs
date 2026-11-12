use super::context::FnEmitter;
use super::small_utils::assign_bin_op;
use std::sync::Arc;
use vn_core::ast::operators::AssignOp;
use vn_core::ast::ExprId;
use vn_tir::{
    BackendTy, DynReason, Resolution, Span, TirArg, TirExpr, TirExprKind, TirStmt, TirUnOp,
};

#[derive(Clone)]
pub(super) enum AssignValue {
    Expr(ExprId),
    Lowered(TirExpr),
}

impl<'a> FnEmitter<'a> {
    pub(super) fn assign_value(&mut self, value: AssignValue) -> TirExpr {
        match value {
            AssignValue::Expr(e) => self.lower_expr(e),
            AssignValue::Lowered(t) => t,
        }
    }

    pub(crate) fn this_field_assign(&mut self, field: Arc<str>, value: TirExpr) -> TirStmt {
        let this = self.this_var(Span::EMPTY);
        let target = self.field_place(
            this,
            field,
            BackendTy::Dynamic(DynReason::Unrecorded),
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

    pub(crate) fn this_param_field_assign(
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
        value: AssignValue,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let mangled = self.m.desugar.extension_set_members
            [&self.ast_arena.expr(target).range.start.offset]
            .clone();
        let recv = self.lower_expr(match &self.ast_arena.expr(target).kind {
            vn_core::ast::ExprKind::Member { object, .. } => *object,
            vn_core::ast::ExprKind::IntLiteral { .. }
            | vn_core::ast::ExprKind::FloatLiteral { .. }
            | vn_core::ast::ExprKind::BigIntLiteral { .. }
            | vn_core::ast::ExprKind::DecimalLiteral { .. }
            | vn_core::ast::ExprKind::StrLiteral { .. }
            | vn_core::ast::ExprKind::CharLiteral { .. }
            | vn_core::ast::ExprKind::BoolLiteral { .. }
            | vn_core::ast::ExprKind::NullLiteral
            | vn_core::ast::ExprKind::Template { .. }
            | vn_core::ast::ExprKind::TaggedTemplate { .. }
            | vn_core::ast::ExprKind::Identifier { .. }
            | vn_core::ast::ExprKind::Missing
            | vn_core::ast::ExprKind::This
            | vn_core::ast::ExprKind::ImportMeta(_)
            | vn_core::ast::ExprKind::Super
            | vn_core::ast::ExprKind::Array { .. }
            | vn_core::ast::ExprKind::Object { .. }
            | vn_core::ast::ExprKind::Tuple { .. }
            | vn_core::ast::ExprKind::Unary { .. }
            | vn_core::ast::ExprKind::Update { .. }
            | vn_core::ast::ExprKind::Binary { .. }
            | vn_core::ast::ExprKind::Logical { .. }
            | vn_core::ast::ExprKind::Assign { .. }
            | vn_core::ast::ExprKind::Conditional { .. }
            | vn_core::ast::ExprKind::Call { .. }
            | vn_core::ast::ExprKind::New { .. }
            | vn_core::ast::ExprKind::Function { .. }
            | vn_core::ast::ExprKind::Arrow { .. }
            | vn_core::ast::ExprKind::Sequence { .. }
            | vn_core::ast::ExprKind::Paren { .. }
            | vn_core::ast::ExprKind::Await { .. }
            | vn_core::ast::ExprKind::Spawn { .. }
            | vn_core::ast::ExprKind::Yield { .. }
            | vn_core::ast::ExprKind::Spread { .. }
            | vn_core::ast::ExprKind::Pipeline { .. }
            | vn_core::ast::ExprKind::Range { .. }
            | vn_core::ast::ExprKind::NonNull { .. }
            | vn_core::ast::ExprKind::Try { .. }
            | vn_core::ast::ExprKind::As { .. }
            | vn_core::ast::ExprKind::Satisfies { .. }
            | vn_core::ast::ExprKind::ClassExpr { .. }
            | vn_core::ast::ExprKind::Match { .. }
            | vn_core::ast::ExprKind::Is { .. }
            | vn_core::ast::ExprKind::With { .. }
            | vn_core::ast::ExprKind::MetaAccess { .. } => unreachable!(),
        });
        let v = self.assign_value(value);
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
        value: AssignValue,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let obj = self.lower_expr(object);
        let index = self.lower_expr(property);
        let v = self.assign_value(value);
        let node_ty = match obj.ty.non_nullable(self.tt) {
            BackendTy::Array(el) => self.tt.get(el),
            BackendTy::Map(_, val) => self.tt.get(val),
            BackendTy::Literal(_)
            | BackendTy::Int
            | BackendTy::Float
            | BackendTy::Bool
            | BackendTy::Char
            | BackendTy::Str
            | BackendTy::Bytes
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Class(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Nullable(_)
            | BackendTy::Void
            | BackendTy::Never
            | BackendTy::Dynamic(_) => v.ty,
        };
        let v = self.as_assigned(op, v, node_ty);
        let read_ty = self.keyed_read_ty(obj.ty, node_ty);
        let index_node = |object: TirExpr, index: TirExpr, ty: BackendTy| TirExpr {
            kind: TirExprKind::Index {
                object: Box::new(object),
                index: Box::new(index),
            },
            ty,
            res: Resolution::None,
            span,
        };
        let index_target = |object: TirExpr, index: TirExpr| index_node(object, index, node_ty);
        let plain = matches!(assign_bin_op(op), Ok(None));
        let (obj_w, index_w) = if plain {
            (obj, index)
        } else {
            (self.pin(obj), self.pin(index))
        };
        let rhs = match assign_bin_op(op) {
            Ok(None) => v,
            Ok(Some(bop)) => {
                let read = index_node(obj_w.clone(), index_w.clone(), read_ty);
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
                let read = index_node(obj_w.clone(), index_w.clone(), read_ty);
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
        value: AssignValue,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        if self.is_value_field_place(target) {
            return self.lower_value_field_assign(op, target, value, span);
        }
        if let Some(assigned) = self.lower_accessor_assign(op, target, value.clone(), span) {
            return assigned;
        }
        self.place_target = true;
        let t = self.lower_expr(target);
        self.place_target = false;
        let t = match assign_bin_op(op) {
            Ok(None) => t,
            _ => self.pin_place(t),
        };
        let v = self.assign_value(value);
        let declared = self.expr_ty(target);
        let v = self.as_assigned(op, v, declared);
        let rhs = self.assigned_rhs(op, t.clone(), v, span);
        let rhs = self.guard_dynamic(rhs, t.ty);
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

    pub(super) fn assigned_rhs(
        &mut self,
        op: AssignOp,
        current: TirExpr,
        v: TirExpr,
        span: Span,
    ) -> TirExpr {
        match assign_bin_op(op) {
            Ok(None) => v,
            Ok(Some(bop)) => {
                let node_ty = current.ty;
                let (lhs, rhs, nty) = self.coerce_binary_operands(bop, current, v, node_ty);
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
            Err(()) => self.lower_logical_assign(op, current, v, span),
        }
    }

    pub(super) fn as_assigned(&self, op: AssignOp, value: TirExpr, place: BackendTy) -> TirExpr {
        match assign_bin_op(op) {
            Ok(Some(_)) => value,
            Ok(None) | Err(()) => self.widened(value, place),
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
            AssignOp::Assign
            | AssignOp::AddAssign
            | AssignOp::SubAssign
            | AssignOp::MulAssign
            | AssignOp::DivAssign
            | AssignOp::ModAssign
            | AssignOp::PowAssign
            | AssignOp::BitAndAssign
            | AssignOp::BitOrAssign
            | AssignOp::BitXorAssign
            | AssignOp::ShlAssign
            | AssignOp::ShrAssign
            | AssignOp::UShrAssign
            | AssignOp::AndAssign
            | AssignOp::OrAssign => self.cast_to(read.clone(), BackendTy::Bool),
        };
        let v = self.cast_to(v, place_ty);
        let (then_val, else_val) = match op {
            AssignOp::OrAssign => (read, v),
            AssignOp::Assign
            | AssignOp::AddAssign
            | AssignOp::SubAssign
            | AssignOp::MulAssign
            | AssignOp::DivAssign
            | AssignOp::ModAssign
            | AssignOp::PowAssign
            | AssignOp::BitAndAssign
            | AssignOp::BitOrAssign
            | AssignOp::BitXorAssign
            | AssignOp::ShlAssign
            | AssignOp::ShrAssign
            | AssignOp::UShrAssign
            | AssignOp::AndAssign
            | AssignOp::NullishAssign => (v, read),
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
