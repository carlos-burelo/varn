use super::Checker;
use varn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(super) fn has_builtin(
        &self,
        decorators: &[varn_core::ast::Decorator],
        bind: &BindResult,
        want: &str,
    ) -> bool {
        decorators.iter().any(|d| {
            if bind.user_decorators.contains(&d.range.start.offset) {
                return false;
            }
            let expr = &self.ast_arena.expr(d.expression);
            let head = match &expr.kind {
                varn_core::ast::ExprKind::Identifier { name } => Some(*name),
                varn_core::ast::ExprKind::Call { callee, .. } => {
                    match &self.ast_arena.expr(*callee).kind {
                        varn_core::ast::ExprKind::Identifier { name } => Some(*name),
                        varn_core::ast::ExprKind::IntLiteral { .. }
                        | varn_core::ast::ExprKind::FloatLiteral { .. }
                        | varn_core::ast::ExprKind::BigIntLiteral { .. }
                        | varn_core::ast::ExprKind::DecimalLiteral { .. }
                        | varn_core::ast::ExprKind::StrLiteral { .. }
                        | varn_core::ast::ExprKind::CharLiteral { .. }
                        | varn_core::ast::ExprKind::BoolLiteral { .. }
                        | varn_core::ast::ExprKind::NullLiteral
                        | varn_core::ast::ExprKind::RegexLiteral { .. }
                        | varn_core::ast::ExprKind::Template { .. }
                        | varn_core::ast::ExprKind::TaggedTemplate { .. }
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
                varn_core::ast::ExprKind::IntLiteral { .. }
                | varn_core::ast::ExprKind::FloatLiteral { .. }
                | varn_core::ast::ExprKind::BigIntLiteral { .. }
                | varn_core::ast::ExprKind::DecimalLiteral { .. }
                | varn_core::ast::ExprKind::StrLiteral { .. }
                | varn_core::ast::ExprKind::CharLiteral { .. }
                | varn_core::ast::ExprKind::BoolLiteral { .. }
                | varn_core::ast::ExprKind::NullLiteral
                | varn_core::ast::ExprKind::RegexLiteral { .. }
                | varn_core::ast::ExprKind::Template { .. }
                | varn_core::ast::ExprKind::TaggedTemplate { .. }
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
            };
            head.is_some_and(|h| bind.interner.resolve(h) == want)
        })
    }

    pub(super) fn check_inline_shape(&mut self, f: &varn_core::ast::FunctionDecl, name: &str) {
        let mut reason: Option<&str> = None;
        if f.modifiers.is_async {
            reason = Some("async functions");
        } else if f.modifiers.is_generator {
            reason = Some("generators");
        } else if f.params.iter().any(|p| p.is_rest) {
            reason = Some("rest parameters");
        } else {
            match &self.ast_arena.stmt(f.body).kind {
                varn_core::ast::StmtKind::Block { stmts } if stmts.len() == 1 => {
                    match &self.ast_arena.stmt(stmts[0]).kind {
                        varn_core::ast::StmtKind::Return {
                            argument: Some(arg),
                        } => match &self.ast_arena.expr(*arg).kind {
                            varn_core::ast::ExprKind::Arrow { .. }
                            | varn_core::ast::ExprKind::Function { .. }
                            | varn_core::ast::ExprKind::ClassExpr { .. } => {
                                reason = Some("closures returned by value");
                            }
                            varn_core::ast::ExprKind::IntLiteral { .. }
                            | varn_core::ast::ExprKind::FloatLiteral { .. }
                            | varn_core::ast::ExprKind::BigIntLiteral { .. }
                            | varn_core::ast::ExprKind::DecimalLiteral { .. }
                            | varn_core::ast::ExprKind::StrLiteral { .. }
                            | varn_core::ast::ExprKind::CharLiteral { .. }
                            | varn_core::ast::ExprKind::BoolLiteral { .. }
                            | varn_core::ast::ExprKind::NullLiteral
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
                            | varn_core::ast::ExprKind::Match { .. }
                            | varn_core::ast::ExprKind::Is { .. }
                            | varn_core::ast::ExprKind::With { .. }
                            | varn_core::ast::ExprKind::MetaAccess { .. } => {}
                        },
                        varn_core::ast::StmtKind::Block { .. }
                        | varn_core::ast::StmtKind::Empty
                        | varn_core::ast::StmtKind::Expr { .. }
                        | varn_core::ast::StmtKind::Decl(_)
                        | varn_core::ast::StmtKind::Error
                        | varn_core::ast::StmtKind::If { .. }
                        | varn_core::ast::StmtKind::While { .. }
                        | varn_core::ast::StmtKind::DoWhile { .. }
                        | varn_core::ast::StmtKind::For { .. }
                        | varn_core::ast::StmtKind::ForIn { .. }
                        | varn_core::ast::StmtKind::ForOf { .. }
                        | varn_core::ast::StmtKind::Switch { .. }
                        | varn_core::ast::StmtKind::Return { .. }
                        | varn_core::ast::StmtKind::Break { .. }
                        | varn_core::ast::StmtKind::Continue { .. }
                        | varn_core::ast::StmtKind::Throw { .. }
                        | varn_core::ast::StmtKind::Try { .. }
                        | varn_core::ast::StmtKind::Using { .. }
                        | varn_core::ast::StmtKind::Labeled { .. }
                        | varn_core::ast::StmtKind::Debugger => {
                            reason = Some("bodies that are not a single `return`")
                        }
                    }
                }
                varn_core::ast::StmtKind::Block { .. }
                | varn_core::ast::StmtKind::Empty
                | varn_core::ast::StmtKind::Expr { .. }
                | varn_core::ast::StmtKind::Decl(_)
                | varn_core::ast::StmtKind::Error
                | varn_core::ast::StmtKind::If { .. }
                | varn_core::ast::StmtKind::While { .. }
                | varn_core::ast::StmtKind::DoWhile { .. }
                | varn_core::ast::StmtKind::For { .. }
                | varn_core::ast::StmtKind::ForIn { .. }
                | varn_core::ast::StmtKind::ForOf { .. }
                | varn_core::ast::StmtKind::Switch { .. }
                | varn_core::ast::StmtKind::Return { .. }
                | varn_core::ast::StmtKind::Break { .. }
                | varn_core::ast::StmtKind::Continue { .. }
                | varn_core::ast::StmtKind::Throw { .. }
                | varn_core::ast::StmtKind::Try { .. }
                | varn_core::ast::StmtKind::Using { .. }
                | varn_core::ast::StmtKind::Labeled { .. }
                | varn_core::ast::StmtKind::Debugger => {
                    reason = Some("bodies that are not a single `return`")
                }
            }
        }
        if let Some(what) = reason {
            self.emit(
                varn_core::diagnostics::Diagnostic::error(
                    varn_core::diagnostics::ErrorCode::InvalidDecoratorSignature,
                    format!("`@inline` cannot apply to {what} (function '{name}')"),
                )
                .with_range(f.range),
            );
        }
    }
}
