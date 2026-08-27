use std::rc::Rc;

use varn_core::ast::expr::ArrowBody;
use varn_core::ast::{Expr, ExprKind};

use super::*;

impl<'a> Lowerer<'a> {
    /// Function, arrow and class expressions: the forms that lower to a nested
    /// HIR function or class rather than to instructions.
    pub(super) fn lower_function_expr(&mut self, expr: &Expr, scope: &mut Scope) -> R<HirExpr> {
        match &expr.kind {
            ExprKind::Function {
                fn_id,
                params,
                body,
                is_async,
                is_generator,
                ..
            } => {
                let name = fn_id.clone().unwrap_or_else(|| Rc::from("<anon>"));
                let return_ty = self.value_ty(AnnKey::expr(expr.id));
                let (func, upvalues) = self.lower_function_like(
                    name,
                    params,
                    *is_async,
                    *is_generator,
                    false,
                    false,
                    expr.range.start.line,
                    BodyRef::Block(body),
                    &[],
                    return_ty,
                    scope,
                )?;
                Ok(HirExpr::Closure {
                    func: Box::new(func),
                    upvalues,
                })
            }
            ExprKind::Arrow {
                params,
                body,
                is_async,
                ..
            } => {
                let body_ref = match body.as_ref() {
                    ArrowBody::Expr(e) => BodyRef::ExprBody(e),
                    ArrowBody::Block(s) => BodyRef::Block(s),
                };
                let return_ty = self.value_ty(AnnKey::expr(expr.id));
                let (func, upvalues) = self.lower_function_like(
                    Rc::from("<arrow>"),
                    params,
                    *is_async,
                    false,
                    false,
                    false,
                    expr.range.start.line,
                    body_ref,
                    &[],
                    return_ty,
                    scope,
                )?;
                Ok(HirExpr::Closure {
                    func: Box::new(func),
                    upvalues,
                })
            }
            ExprKind::ClassExpr { declaration } => {
                let hir_class = self.lower_class(declaration, scope)?;
                Ok(HirExpr::Class(Box::new(hir_class)))
            }
            other => unreachable!("lower_function_expr: {other:?} is not handled here"),
        }
    }
}
