//! Operators on user types (spec §34): `a + b` where `a`'s type declares the
//! capability method is `a.add(b)`, resolved here and recorded in
//! `Desugarings::operator_calls` for the emitter.

use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::{ObjectTypeMember, Type};
use varn_core::ast::ExprId;
use varn_core::capability::{OperatorMethod, OperatorShape};
use varn_core::{Diagnostic, ErrorCode, TypeKind};

/// What a capability method makes of an operator.
pub(crate) struct ResolvedOperator {
    /// The method's parameter, for a binary operator.
    pub param: Option<Type>,
    /// The operator's value: the method's return, or `bool` for comparisons.
    pub result: Type,
}

impl Checker<'_> {
    /// The capability method `recv` answers `op` with, when `recv` is a user
    /// type (class, interface, object) declaring it. Primitives, unions and
    /// `dynamic` keep their built-in operators.
    pub(crate) fn resolve_operator(
        &mut self,
        recv: &Type,
        op: OperatorMethod,
        bind: &BindResult,
    ) -> Option<ResolvedOperator> {
        if !matches!(
            self.ty_table.get(recv.0),
            TypeKind::Named(..) | TypeKind::Generic(..) | TypeKind::Object(_)
        ) {
            return None;
        }
        let member = self.find_member(recv, op.method, bind)?;
        let fn_ty = match member {
            ObjectTypeMember::Method {
                params,
                return_type,
                ..
            } => (
                params.first().map(|p| Type(p.ty, false)),
                Type(return_type, false),
            ),
            ObjectTypeMember::Property { ty, .. } => match self.ty_table.get(ty) {
                TypeKind::Fn(fid) => {
                    let ft = self.ty_table.get_function(fid);
                    (
                        ft.params.first().map(|p| Type(p.ty, false)),
                        Type(ft.return_type, false),
                    )
                }
                _ => return None,
            },
            _ => return None,
        };
        let (param, ret) = fn_ty;
        let result = match op.shape {
            OperatorShape::Value => ret,
            OperatorShape::CompareToZero(_) | OperatorShape::Equals | OperatorShape::NotEquals => {
                Type::Bool
            }
        };
        Some(ResolvedOperator { param, result })
    }

    /// Resolve `left <op> right` through `left`'s capability method. Returns
    /// whether it did; the built-in operator rules then do not apply.
    pub(super) fn check_binary_capability(
        &mut self,
        expr: ExprId,
        op: varn_core::ast::operators::BinaryOp,
        left: ExprId,
        right: ExprId,
        bind: &BindResult,
    ) -> bool {
        let Some(method) = varn_core::capability::binary_operator_method(op) else {
            return false;
        };
        let l_ty = self.infer_type(left, bind);
        let r_ty = self.infer_type(right, bind);
        // `x == null` stays the null test.
        if matches!(
            method.shape,
            OperatorShape::Equals | OperatorShape::NotEquals
        ) && r_ty == Type::Null
        {
            return false;
        }
        let Some(resolved) = self.resolve_operator(&l_ty, method, bind) else {
            return false;
        };
        if let Some(param) = resolved.param {
            if !self.value_assignable_to(&param, &r_ty, Some(right), Some(bind)) {
                let recv_s = l_ty.display(&self.ty_table, &bind.interner);
                let param_s = param.display(&self.ty_table, &bind.interner);
                let arg_s = r_ty.display(&self.ty_table, &bind.interner);
                self.emit(
                    Diagnostic::error(
                        ErrorCode::InvalidTypeOperator,
                        format!(
                            "operator resolves to '{recv_s}.{}({param_s})', which does not accept '{arg_s}'",
                            method.method
                        ),
                    )
                    .with_range(self.ast_arena.expr(expr).range),
                );
            }
        }
        self.desugar.operator_calls.insert(expr.index());
        true
    }

    /// `-x` through `x`'s `neg()`.
    pub(super) fn check_unary_capability(
        &mut self,
        expr: ExprId,
        op: varn_core::ast::operators::UnaryOp,
        operand: ExprId,
        bind: &BindResult,
    ) {
        let Some(method) = varn_core::capability::unary_operator_method(op) else {
            return;
        };
        let ty = self.infer_type(operand, bind);
        if self.resolve_operator(&ty, method, bind).is_some() {
            self.desugar.operator_calls.insert(expr.index());
        }
    }
}
