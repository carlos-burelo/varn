use super::super::Checker;
use crate::binder::BindResult;
use varn_core::ast::{AstArena, ExprId, SwitchCase};
use varn_core::{Diagnostic, ErrorCode};

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
        _ => None,
    }
}
