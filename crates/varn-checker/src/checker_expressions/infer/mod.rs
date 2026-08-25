mod collectors;
mod infer_call;
mod infer_impl;
pub(crate) mod member_binary;

use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::Type;
use varn_core::ast::{Expr, ExprKind};

pub(crate) use self::collectors::collect_checked_return_types;

impl<'r> Checker<'r> {
    pub(crate) fn infer_type_internal(&mut self, expr: &Expr, bind: &BindResult) -> Type {
        if let ExprKind::Identifier { name } = &expr.kind {
            let scope = bind.scopes.get(self.current_scope);
            if let Some(id) = scope.resolve(name, &bind.scopes) {
                if let Some(stack) = self.narrowed_types.get(&id) {
                    if let Some(ty) = stack.last() {
                        return ty.clone();
                    }
                }
                if let Some(ty) = self.symbol_types.get(&id).cloned() {
                    return ty;
                }
            }
        }

        if let ExprKind::NonNull { expression } = &expr.kind {
            return self.infer_type(expression, bind).non_nullified();
        }

        let ty = self.infer_type_impl(expr, bind);
        let is_opt_call = matches!(
            &expr.kind,
            ExprKind::Call {
                callee,
                optional: false,
                ..
            } if matches!(&callee.kind, ExprKind::Member { optional: true, .. })
        );

        match &expr.kind {
            ExprKind::Member { optional: true, .. } => Type::make_nullable(ty),
            ExprKind::Call { optional: true, .. } => Type::make_nullable(ty),
            _ if is_opt_call => Type::make_nullable(ty),
            _ => ty,
        }
    }
}
