use super::super::Checker;
use crate::checker::recorder::Recorder;
use varn_core::ast::{AstArena, ExprId, StmtId, StmtKind, SwitchCase};
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(super) fn check_switch_stmt(
        &mut self,
        rec: &mut Recorder,
        discriminant: ExprId,
        cases: Vec<SwitchCase>,
        bind: &BindResult,
    ) {
        self.check_expr(rec, discriminant, bind);
        self.switch_depth += 1;
        let mut seen_cases = rustc_hash::FxHashSet::default();
        for (at, case) in cases.iter().enumerate() {
            self.check_case_shape(case, at + 1 == cases.len());
            if let Some(t) = case.test {
                self.check_expr(rec, t, bind);
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
            self.check_stmts(rec, &case.body, bind);
        }
        self.switch_depth -= 1;
    }
}

impl<'r> Checker<'r> {
    fn check_case_shape(&mut self, case: &SwitchCase, is_last: bool) {
        let arena = self.ast_arena;
        let reject = |c: &mut Self, message: &str, range| {
            c.emit(Diagnostic::error(ErrorCode::InvalidSwitchCase, message).with_range(range));
        };
        if case.test.is_none() && !is_last {
            reject(
                self,
                "'default' must be the last case of a switch",
                case.range,
            );
        }
        if case.body.is_empty() && !is_last {
            reject(
                self,
                "an empty case does nothing: a case never falls through to the next one. Use 'match' with 'a | b' to share a body",
                case.range,
            );
        }
        let closing = case
            .body
            .last()
            .copied()
            .filter(|s| matches!(arena.stmt(*s).kind, StmtKind::Break { label: None }));
        let mut stray = Vec::new();
        for stmt in &case.body {
            if Some(*stmt) != closing {
                stray_breaks(*stmt, arena, &mut stray);
            }
        }
        for stmt in stray {
            self.emit(
                Diagnostic::error(
                    ErrorCode::InvalidBreakTarget,
                    "'break' can only close a switch case as its last statement: cases never fall through, so nothing else needs it",
                )
                .with_range(arena.stmt(stmt).range),
            );
        }
    }
}

fn stray_breaks(stmt: StmtId, arena: &AstArena, out: &mut Vec<StmtId>) {
    match &arena.stmt(stmt).kind {
        StmtKind::Break { label: None } => out.push(stmt),
        StmtKind::Break { label: Some(_) } => {}
        StmtKind::Block { stmts } => {
            for s in stmts {
                stray_breaks(*s, arena, out);
            }
        }
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => {
            stray_breaks(*consequent, arena, out);
            if let Some(alt) = alternate {
                stray_breaks(*alt, arena, out);
            }
        }
        StmtKind::Try {
            block,
            catches,
            finally,
        } => {
            stray_breaks(*block, arena, out);
            for c in catches {
                stray_breaks(c.body, arena, out);
            }
            if let Some(f) = finally {
                stray_breaks(*f, arena, out);
            }
        }
        StmtKind::Labeled { body, .. } => stray_breaks(*body, arena, out),
        StmtKind::Empty
        | StmtKind::Expr { .. }
        | StmtKind::Decl(_)
        | StmtKind::Error
        | StmtKind::While { .. }
        | StmtKind::DoWhile { .. }
        | StmtKind::For { .. }
        | StmtKind::ForIn { .. }
        | StmtKind::ForOf { .. }
        | StmtKind::Switch { .. }
        | StmtKind::Return { .. }
        | StmtKind::Continue { .. }
        | StmtKind::Throw { .. }
        | StmtKind::Using { .. }
        | StmtKind::Debugger => {}
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
