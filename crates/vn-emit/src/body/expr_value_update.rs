use super::context::FnEmitter;
use super::expr_assign::AssignValue;
use super::small_utils::assign_bin_op;
use std::sync::Arc;
use vn_core::ast::operators::AssignOp;
use vn_core::ast::{ExprId, ExprKind};
use vn_tir::{BackendTy, Resolution, Span, TirExpr, TirExprKind, TirObjectEntry, TirStmt};

impl<'a> FnEmitter<'a> {
    pub(super) fn is_value_field_place(&self, expr: ExprId) -> bool {
        self.m.desugar.value_field_places.contains(&expr.index())
    }

    pub(super) fn lower_value_field_assign(
        &mut self,
        op: AssignOp,
        target: ExprId,
        value: AssignValue,
        span: Span,
    ) -> TirExpr {
        let declared = self.expr_ty(target);
        let current = match assign_bin_op(op) {
            Ok(None) => None,
            Ok(Some(_)) | Err(()) => Some(self.lower_expr(target)),
        };
        let v = self.assign_value(value);
        let v = self.as_assigned(op, v, declared);
        let rhs = match current {
            None => v,
            Some(current) => self.assigned_rhs(op, current, v, span),
        };
        let rhs = self.guard_dynamic(rhs, declared);
        let rhs = self.pin(rhs);
        self.store_value_field(target, rhs.clone(), span);
        rhs
    }

    pub(super) fn store_value_field(&mut self, target: ExprId, value: TirExpr, span: Span) {
        let ExprKind::Member {
            object, property, ..
        } = &self.ast_arena.expr(target).kind
        else {
            return;
        };
        let (object, property) = (*object, *property);
        let ExprKind::Identifier { name } = &self.ast_arena.expr(property).kind else {
            return;
        };
        let name: Arc<str> = Arc::from(self.m.interner.resolve(*name));
        let source = self.lower_expr(object);
        let updated = self.with_field(source, name, value, span);
        if self.is_value_field_place(object) {
            let updated = self.pin(updated);
            self.store_value_field(object, updated, span);
            return;
        }
        let void = BackendTy::Void;
        let assigned = match &self.ast_arena.expr(object).kind {
            ExprKind::Member {
                object: container,
                property: index,
                computed: true,
                ..
            } => {
                let (container, index) = (*container, *index);
                self.lower_index_assign(
                    AssignOp::Assign,
                    container,
                    index,
                    AssignValue::Lowered(updated),
                    void,
                    span,
                )
            }
            ExprKind::Identifier { .. } | ExprKind::Member { .. } => self.lower_plain_assign(
                AssignOp::Assign,
                object,
                AssignValue::Lowered(updated),
                void,
                span,
            ),
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::ImportMeta(_)
            | ExprKind::Super
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Assign { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::Call { .. }
            | ExprKind::New { .. }
            | ExprKind::Function { .. }
            | ExprKind::Arrow { .. }
            | ExprKind::Sequence { .. }
            | ExprKind::Paren { .. }
            | ExprKind::Await { .. }
            | ExprKind::Spawn { .. }
            | ExprKind::Yield { .. }
            | ExprKind::Spread { .. }
            | ExprKind::Pipeline { .. }
            | ExprKind::Range { .. }
            | ExprKind::NonNull { .. }
            | ExprKind::Try { .. }
            | ExprKind::As { .. }
            | ExprKind::Satisfies { .. }
            | ExprKind::ClassExpr { .. }
            | ExprKind::Match { .. }
            | ExprKind::Is { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. } => unreachable!(),
        };
        self.pending.push(TirStmt::Expr(assigned));
    }

    fn with_field(
        &mut self,
        source: TirExpr,
        name: Arc<str>,
        value: TirExpr,
        span: Span,
    ) -> TirExpr {
        let source = self.pin(source);
        let ty = source.ty;
        if let BackendTy::Class(class) = ty.non_nullable(self.tt)
            && self
                .m
                .classes
                .get(class.0 as usize)
                .is_some_and(|info| info.layout.is_value)
        {
            return self.value_with_given(class, source, vec![(name, value)], ty, span);
        }
        TirExpr {
            kind: TirExprKind::ObjectLit {
                entries: vec![
                    TirObjectEntry::Spread(source),
                    TirObjectEntry::Field { name, value },
                ],
            },
            ty,
            res: Resolution::None,
            span,
        }
    }
}
