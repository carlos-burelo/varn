use crate::checker::Checker;
use varn_core::ast::operators::UnaryOp;
use varn_core::ast::{ExprId, ExprKind};
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn extract_narrowings(
        &mut self,
        expr: ExprId,
        bind: &BindResult,
        is_true_branch: bool,
    ) -> Vec<(varn_sem::symbol::SymbolId, Type)> {
        let arena = self.ast_arena;
        let cache_key = (expr.index(), is_true_branch, self.current_scope);
        if let Some(cached) = self.narrowings_cache.get(&cache_key) {
            return cached.clone();
        }

        let mut narrowings = Vec::new();

        match &arena.expr(expr).kind {
            ExprKind::Unary {
                op: UnaryOp::Not,
                operand,
                ..
            } => {
                let operand = *operand;
                narrowings.extend(self.extract_narrowings(operand, bind, !is_true_branch));
            }

            ExprKind::Identifier { name } => {
                self.narrow_identifier(*name, bind, is_true_branch, &mut narrowings);
            }

            ExprKind::Binary { left, right, op } => {
                let (left, right, op) = (*left, *right, *op);
                self.narrow_typeof(left, right, op, bind, is_true_branch, &mut narrowings);
                self.narrow_null_comparison(left, right, op, bind, is_true_branch, &mut narrowings);
                self.narrow_discriminant(left, right, op, bind, is_true_branch, &mut narrowings);
                self.narrow_instanceof(left, right, op, bind, is_true_branch, &mut narrowings);
            }

            ExprKind::Logical {
                left,
                right,
                op: varn_core::ast::operators::LogicalOp::And,
            } => {
                self.narrow_logical_and(*left, *right, bind, is_true_branch, &mut narrowings);
            }

            ExprKind::Logical {
                left,
                right,
                op: varn_core::ast::operators::LogicalOp::Or,
            } => {
                self.narrow_logical_or(*left, *right, bind, is_true_branch, &mut narrowings);
            }

            ExprKind::Is {
                expression,
                type_ann,
            } => {
                let (expression, type_ann) = (*expression, type_ann.clone());
                self.narrow_is(expression, type_ann, bind, is_true_branch, &mut narrowings);
            }

            ExprKind::Call { callee, args, .. } => {
                let (callee, args) = (*callee, args.clone());
                self.narrow_type_guard(callee, &args, bind, is_true_branch, &mut narrowings);
            }

            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::RegexLiteral { .. }
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Record { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Assign { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::Member { .. }
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
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. } => {}
        }
        self.narrowings_cache.insert(cache_key, narrowings.clone());
        narrowings
    }
}
