use super::super::Checker;
use varn_core::ast::{AstArena, ExprId, SwitchCase};
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(super) fn check_switch_stmt(
        &mut self,
        discriminant: ExprId,
        cases: Vec<SwitchCase>,
        bind: &BindResult,
    ) {
        self.check_expr(discriminant, bind);
        self.switch_depth += 1;
        let mut seen_cases = rustc_hash::FxHashSet::default();
        for case in &cases {
            if let Some(t) = case.test {
                self.check_expr(t, bind);
                if let Some(lit_val) = get_literal_value_key(t, self.ast_arena) {
                    if !seen_cases.insert(lit_val.clone()) {
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::DuplicateCaseLabel,
                                format!("duplicate case label '{}'", lit_val),
                            )
                            .with_range(self.ast_arena.expr(t).range),
                        );
                    }
                }
            }
            self.check_stmts(&case.body, bind);
        }
        self.switch_depth -= 1;
    }
}

fn get_literal_value_key(expr: ExprId, arena: &AstArena) -> Option<String> {
    match &arena.expr(expr).kind {
        varn_core::ast::ExprKind::IntLiteral { value, .. } => Some(value.to_string()),
        varn_core::ast::ExprKind::FloatLiteral { value, .. } => Some(value.to_string()),
        varn_core::ast::ExprKind::StrLiteral { value } => Some(format!("\"{}\"", value)),
        varn_core::ast::ExprKind::BoolLiteral { value } => Some(value.to_string()),
        varn_core::ast::ExprKind::CharLiteral { value } => Some(format!("'{}'", value)),
        varn_core::ast::ExprKind::NullLiteral => Some("null".to_owned()),
        varn_core::ast::ExprKind::BigIntLiteral { .. }
        | varn_core::ast::ExprKind::DecimalLiteral { .. }
        | varn_core::ast::ExprKind::RegexLiteral { .. }
        | varn_core::ast::ExprKind::Template { .. }
        | varn_core::ast::ExprKind::TaggedTemplate { .. }
        | varn_core::ast::ExprKind::Identifier { .. }
        | varn_core::ast::ExprKind::Missing
        | varn_core::ast::ExprKind::This
        | varn_core::ast::ExprKind::Super
        | varn_core::ast::ExprKind::Array { .. }
        | varn_core::ast::ExprKind::Object { .. }
        | varn_core::ast::ExprKind::Tuple { .. }
        | varn_core::ast::ExprKind::Record { .. }
        | varn_core::ast::ExprKind::Unary { .. }
        | varn_core::ast::ExprKind::Update { .. }
        | varn_core::ast::ExprKind::Binary { .. }
        | varn_core::ast::ExprKind::Logical { .. }
        | varn_core::ast::ExprKind::Assign { .. }
        | varn_core::ast::ExprKind::Conditional { .. }
        | varn_core::ast::ExprKind::Member { .. }
        | varn_core::ast::ExprKind::Call { .. }
        | varn_core::ast::ExprKind::New { .. }
        | varn_core::ast::ExprKind::Function { .. }
        | varn_core::ast::ExprKind::Arrow { .. }
        | varn_core::ast::ExprKind::Sequence { .. }
        | varn_core::ast::ExprKind::Paren { .. }
        | varn_core::ast::ExprKind::Await { .. }
        | varn_core::ast::ExprKind::Spawn { .. }
        | varn_core::ast::ExprKind::Yield { .. }
        | varn_core::ast::ExprKind::Spread { .. }
        | varn_core::ast::ExprKind::Pipeline { .. }
        | varn_core::ast::ExprKind::Range { .. }
        | varn_core::ast::ExprKind::NonNull { .. }
        | varn_core::ast::ExprKind::Try { .. }
        | varn_core::ast::ExprKind::As { .. }
        | varn_core::ast::ExprKind::Satisfies { .. }
        | varn_core::ast::ExprKind::ClassExpr { .. }
        | varn_core::ast::ExprKind::Match { .. }
        | varn_core::ast::ExprKind::Is { .. }
        | varn_core::ast::ExprKind::With { .. }
        | varn_core::ast::ExprKind::MetaAccess { .. } => None,
    }
}
