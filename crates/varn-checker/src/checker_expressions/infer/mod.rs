mod array_ty;
mod collectors;
mod compound;
mod infer_call;
mod infer_impl;
mod logical;
pub(crate) mod member_binary;
mod meta;
mod new_ty;

use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::Type;
use varn_core::ast::{ExprId, ExprKind};

pub(crate) use self::collectors::arrow_body_return_type;

impl<'r> Checker<'r> {
    pub(crate) fn infer_type_internal(&mut self, expr: ExprId, bind: &BindResult) -> Type {
        let arena = self.ast_arena;
        if let ExprKind::Identifier { name } = &arena.expr(expr).kind {
            let scope = bind.scopes.get(self.current_scope);
            if let Some(id) = scope.resolve(*name, &bind.scopes) {
                if let Some(stack) = self.narrowed_types.get(&id) {
                    if let Some(ty) = stack.last() {
                        return *ty;
                    }
                }
                if let Some(ty) = self.symbol_types.get(&id).cloned() {
                    return ty;
                }
            }
        }

        if let ExprKind::NonNull { expression } = &arena.expr(expr).kind {
            let inner = self.infer_type(*expression, bind);
            return inner.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        }

        let ty = self.infer_type_impl(expr, bind);
        let is_opt_call = matches!(
            &arena.expr(expr).kind,
            ExprKind::Call {
                callee,
                optional: false,
                ..
            } if matches!(&arena.expr(*callee).kind, ExprKind::Member { optional: true, .. })
        );

        match &arena.expr(expr).kind {
            ExprKind::Member { optional: true, .. } => {
                Type::make_nullable(ty, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
            }
            ExprKind::Call { optional: true, .. } => {
                Type::make_nullable(ty, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
            }
            _ if is_opt_call => {
                Type::make_nullable(ty, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
            }
            ExprKind::IntLiteral { .. } | ExprKind::FloatLiteral { .. } | ExprKind::BigIntLiteral { .. } | ExprKind::DecimalLiteral { .. } | ExprKind::StrLiteral { .. } | ExprKind::CharLiteral { .. } | ExprKind::BoolLiteral { .. } | ExprKind::NullLiteral | ExprKind::RegexLiteral { .. } | ExprKind::Template { .. } | ExprKind::TaggedTemplate { .. } | ExprKind::Identifier { .. } | ExprKind::Missing | ExprKind::This | ExprKind::Super | ExprKind::Array { .. } | ExprKind::Object { .. } | ExprKind::Tuple { .. } | ExprKind::Record { .. } | ExprKind::Unary { .. } | ExprKind::Update { .. } | ExprKind::Binary { .. } | ExprKind::Logical { .. } | ExprKind::Assign { .. } | ExprKind::Conditional { .. } | ExprKind::Member { .. } | ExprKind::Call { .. } | ExprKind::New { .. } | ExprKind::Function { .. } | ExprKind::Arrow { .. } | ExprKind::Sequence { .. } | ExprKind::Paren { .. } | ExprKind::Await { .. } | ExprKind::Spawn { .. } | ExprKind::Yield { .. } | ExprKind::Spread { .. } | ExprKind::Pipeline { .. } | ExprKind::Range { .. } | ExprKind::NonNull { .. } | ExprKind::Try { .. } | ExprKind::As { .. } | ExprKind::Satisfies { .. } | ExprKind::ClassExpr { .. } | ExprKind::Match { .. } | ExprKind::Is { .. } | ExprKind::With { .. } | ExprKind::MetaAccess { .. } => ty,
        }
    }
}
