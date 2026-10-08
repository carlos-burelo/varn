use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::{ObjectTypeMember, Type};
use varn_core::ast::ExprId;
use varn_core::capability::{OperatorMethod, OperatorShape};
use varn_core::{Diagnostic, ErrorCode, TypeKind};

pub(crate) struct ResolvedOperator {
    pub param: Option<Type>,

    pub result: Type,
}

impl Checker<'_> {
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
                params.first().map(|p| Type::resolved(p.ty)),
                Type::resolved(return_type),
            ),
            ObjectTypeMember::Property { ty, .. } => match self.ty_table.get(ty) {
                TypeKind::Fn(fid) => {
                    let ft = self.ty_table.get_function(fid);
                    (
                        ft.params.first().map(|p| Type::resolved(p.ty)),
                        Type::resolved(ft.return_type),
                    )
                }
                TypeKind::Primitive(_)
                | TypeKind::Builtin(_)
                | TypeKind::Literal(_)
                | TypeKind::This
                | TypeKind::Array(_)
                | TypeKind::Union(_)
                | TypeKind::Intersection(_)
                | TypeKind::Tuple(_)
                | TypeKind::Named(..)
                | TypeKind::Generic(..)
                | TypeKind::TemplateLiteral(_)
                | TypeKind::Object(_)
                | TypeKind::Typeof(_)
                | TypeKind::KeyOf(_)
                | TypeKind::IndexedAccess { .. }
                | TypeKind::Mapped { .. }
                | TypeKind::Conditional { .. }
                | TypeKind::Infer(_)
                | TypeKind::EnumVariant { .. }
                | TypeKind::TypePredicate { .. } => return None,
            },
            ObjectTypeMember::Index { .. } | ObjectTypeMember::Callable { .. } => return None,
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
