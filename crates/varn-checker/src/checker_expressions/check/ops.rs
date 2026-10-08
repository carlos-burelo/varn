use super::const_int::overflows_int_literal;
use super::Checker;
use crate::binder::BindResult;
use crate::types::TypeContext;
use varn_core::ast::operators::BinaryOp;
use varn_core::ast::operators::UnaryOp;
use varn_core::ast::{ExprId, ExprKind};

impl<'r> Checker<'r> {
    pub(super) fn check_unary(
        &mut self,
        expr: ExprId,
        op: UnaryOp,
        operand: ExprId,
        arena: &varn_core::ast::AstArena,
        bind: &BindResult,
    ) {
        self.check_expr(operand, bind);
        self.check_unary_capability(expr, op, operand, bind);
        if overflows_int_literal(expr, arena) && !overflows_int_literal(operand, arena) {
            self.report_int_overflow(expr);
        }
    }

    pub(super) fn check_binary(
        &mut self,
        expr: ExprId,
        left: ExprId,
        right: ExprId,
        op: BinaryOp,
        range: varn_core::SourceRange,
        arena: &varn_core::ast::AstArena,
        bind: &BindResult,
    ) {
        self.check_expr(left, bind);
        self.check_expr(right, bind);

        if overflows_int_literal(expr, arena)
            && !overflows_int_literal(left, arena)
            && !overflows_int_literal(right, arena)
        {
            self.report_int_overflow(expr);
        }

        if !self.check_binary_capability(expr, op, left, right, bind) {
            self.check_binary_operands(op, left, right, range, bind);
        }
    }

    pub(super) fn check_new(
        &mut self,
        callee: ExprId,
        args: &[varn_core::ast::Arg],
        range: varn_core::SourceRange,
        arena: &varn_core::ast::AstArena,
        bind: &BindResult,
    ) {
        if self.pure_scope.is_some() {
            self.forbid_pure(
                "allocate with 'new' (constructors cannot prove purity)",
                range,
            );
        }
        let cls_name = match &arena.expr(callee).kind {
            ExprKind::Identifier { name } => Some(bind.interner.resolve(*name)),
            ExprKind::Member {
                property,
                computed: false,
                ..
            } => match &arena.expr(*property).kind {
                ExprKind::Identifier { name } => Some(bind.interner.resolve(*name)),
                _ => None,
            },
            _ => None,
        };
        if let Some(name) = cls_name {
            if self.abstract_classes.contains(name) {
                self.emit(
                    varn_core::Diagnostic::error(
                        varn_core::ErrorCode::AbstractMethodNotImplemented,
                        format!("cannot instantiate abstract class '{name}'"),
                    )
                    .with_range(range),
                );
            }
        }
        self.check_expr(callee, bind);
        let view = crate::binder::BindView::new(bind, self.resolver);
        let ctor_params = cls_name
            .and_then(|cn| {
                view.get_class_members(cn, None).and_then(|members| {
                    members.iter().find_map(|m| {
                        if m.kind == crate::types::ClassMemberKind::Constructor {
                            if let varn_core::TypeKind::Fn(fid) = self.ty_table.get(m.ty.0) {
                                return Some(self.ty_table.get_function(fid).params.clone());
                            }
                        }
                        None
                    })
                })
            })
            .unwrap_or_default();
        self.check_call_args_with_context(args, &ctor_params, bind);
    }
}
