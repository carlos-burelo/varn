use super::Checker;
use vn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(super) fn has_builtin(
        &self,
        decorators: &[vn_core::ast::Decorator],
        bind: &BindResult,
        want: vn_core::ast::decorator_kind::BuiltinDecoratorKind,
    ) -> bool {
        decorators.iter().any(|d| {
            if bind.user_decorators.contains(&d.range.start.offset) {
                return false;
            }
            let expr = &self.ast_arena.expr(d.expression);
            let head = match &expr.kind {
                vn_core::ast::ExprKind::Identifier { name } => Some(*name),
                vn_core::ast::ExprKind::Call { callee, .. } => {
                    match &self.ast_arena.expr(*callee).kind {
                        vn_core::ast::ExprKind::Identifier { name } => Some(*name),
                        vn_core::ast::ExprKind::IntLiteral { .. }
                        | vn_core::ast::ExprKind::FloatLiteral { .. }
                        | vn_core::ast::ExprKind::BigIntLiteral { .. }
                        | vn_core::ast::ExprKind::DecimalLiteral { .. }
                        | vn_core::ast::ExprKind::StrLiteral { .. }
                        | vn_core::ast::ExprKind::CharLiteral { .. }
                        | vn_core::ast::ExprKind::BoolLiteral { .. }
                        | vn_core::ast::ExprKind::NullLiteral
                        | vn_core::ast::ExprKind::Template { .. }
                        | vn_core::ast::ExprKind::TaggedTemplate { .. }
                        | vn_core::ast::ExprKind::Missing
                        | vn_core::ast::ExprKind::This
                        | vn_core::ast::ExprKind::ImportMeta(_)
                        | vn_core::ast::ExprKind::Super
                        | vn_core::ast::ExprKind::Array { .. }
                        | vn_core::ast::ExprKind::Object { .. }
                        | vn_core::ast::ExprKind::Tuple { .. }
                        | vn_core::ast::ExprKind::Unary { .. }
                        | vn_core::ast::ExprKind::Update { .. }
                        | vn_core::ast::ExprKind::Binary { .. }
                        | vn_core::ast::ExprKind::Logical { .. }
                        | vn_core::ast::ExprKind::Assign { .. }
                        | vn_core::ast::ExprKind::Conditional { .. }
                        | vn_core::ast::ExprKind::Member { .. }
                        | vn_core::ast::ExprKind::Call { .. }
                        | vn_core::ast::ExprKind::New { .. }
                        | vn_core::ast::ExprKind::Function { .. }
                        | vn_core::ast::ExprKind::Arrow { .. }
                        | vn_core::ast::ExprKind::Sequence { .. }
                        | vn_core::ast::ExprKind::Paren { .. }
                        | vn_core::ast::ExprKind::Await { .. }
                        | vn_core::ast::ExprKind::Spawn { .. }
                        | vn_core::ast::ExprKind::Yield { .. }
                        | vn_core::ast::ExprKind::Spread { .. }
                        | vn_core::ast::ExprKind::Pipeline { .. }
                        | vn_core::ast::ExprKind::Range { .. }
                        | vn_core::ast::ExprKind::NonNull { .. }
                        | vn_core::ast::ExprKind::Try { .. }
                        | vn_core::ast::ExprKind::As { .. }
                        | vn_core::ast::ExprKind::Satisfies { .. }
                        | vn_core::ast::ExprKind::ClassExpr { .. }
                        | vn_core::ast::ExprKind::Match { .. }
                        | vn_core::ast::ExprKind::Is { .. }
                        | vn_core::ast::ExprKind::With { .. }
                        | vn_core::ast::ExprKind::MetaAccess { .. } => None,
                    }
                }
                vn_core::ast::ExprKind::IntLiteral { .. }
                | vn_core::ast::ExprKind::FloatLiteral { .. }
                | vn_core::ast::ExprKind::BigIntLiteral { .. }
                | vn_core::ast::ExprKind::DecimalLiteral { .. }
                | vn_core::ast::ExprKind::StrLiteral { .. }
                | vn_core::ast::ExprKind::CharLiteral { .. }
                | vn_core::ast::ExprKind::BoolLiteral { .. }
                | vn_core::ast::ExprKind::NullLiteral
                | vn_core::ast::ExprKind::Template { .. }
                | vn_core::ast::ExprKind::TaggedTemplate { .. }
                | vn_core::ast::ExprKind::Missing
                | vn_core::ast::ExprKind::This
                | vn_core::ast::ExprKind::ImportMeta(_)
                | vn_core::ast::ExprKind::Super
                | vn_core::ast::ExprKind::Array { .. }
                | vn_core::ast::ExprKind::Object { .. }
                | vn_core::ast::ExprKind::Tuple { .. }
                | vn_core::ast::ExprKind::Unary { .. }
                | vn_core::ast::ExprKind::Update { .. }
                | vn_core::ast::ExprKind::Binary { .. }
                | vn_core::ast::ExprKind::Logical { .. }
                | vn_core::ast::ExprKind::Assign { .. }
                | vn_core::ast::ExprKind::Conditional { .. }
                | vn_core::ast::ExprKind::Member { .. }
                | vn_core::ast::ExprKind::New { .. }
                | vn_core::ast::ExprKind::Function { .. }
                | vn_core::ast::ExprKind::Arrow { .. }
                | vn_core::ast::ExprKind::Sequence { .. }
                | vn_core::ast::ExprKind::Paren { .. }
                | vn_core::ast::ExprKind::Await { .. }
                | vn_core::ast::ExprKind::Spawn { .. }
                | vn_core::ast::ExprKind::Yield { .. }
                | vn_core::ast::ExprKind::Spread { .. }
                | vn_core::ast::ExprKind::Pipeline { .. }
                | vn_core::ast::ExprKind::Range { .. }
                | vn_core::ast::ExprKind::NonNull { .. }
                | vn_core::ast::ExprKind::Try { .. }
                | vn_core::ast::ExprKind::As { .. }
                | vn_core::ast::ExprKind::Satisfies { .. }
                | vn_core::ast::ExprKind::ClassExpr { .. }
                | vn_core::ast::ExprKind::Match { .. }
                | vn_core::ast::ExprKind::Is { .. }
                | vn_core::ast::ExprKind::With { .. }
                | vn_core::ast::ExprKind::MetaAccess { .. } => None,
            };
            head.is_some_and(|h| {
                vn_core::ast::decorator_kind::BuiltinDecoratorKind::of(bind.interner.resolve(h))
                    == Some(want)
            })
        })
    }

    pub(super) fn check_inline_shape(&mut self, f: &vn_core::ast::FunctionDecl, name: &str) {
        let mut reason: Option<&str> = None;
        if f.modifiers.is_async {
            reason = Some("async functions");
        } else if f.modifiers.is_generator {
            reason = Some("generators");
        } else if f.params.iter().any(|p| p.is_rest) {
            reason = Some("rest parameters");
        } else {
            match &self.ast_arena.stmt(f.body).kind {
                vn_core::ast::StmtKind::Block { stmts } if stmts.len() == 1 => {
                    match &self.ast_arena.stmt(stmts[0]).kind {
                        vn_core::ast::StmtKind::Return {
                            argument: Some(arg),
                        } => match &self.ast_arena.expr(*arg).kind {
                            vn_core::ast::ExprKind::Arrow { .. }
                            | vn_core::ast::ExprKind::Function { .. }
                            | vn_core::ast::ExprKind::ClassExpr { .. } => {
                                reason = Some("closures returned by value");
                            }
                            vn_core::ast::ExprKind::IntLiteral { .. }
                            | vn_core::ast::ExprKind::FloatLiteral { .. }
                            | vn_core::ast::ExprKind::BigIntLiteral { .. }
                            | vn_core::ast::ExprKind::DecimalLiteral { .. }
                            | vn_core::ast::ExprKind::StrLiteral { .. }
                            | vn_core::ast::ExprKind::CharLiteral { .. }
                            | vn_core::ast::ExprKind::BoolLiteral { .. }
                            | vn_core::ast::ExprKind::NullLiteral
                            | vn_core::ast::ExprKind::Template { .. }
                            | vn_core::ast::ExprKind::TaggedTemplate { .. }
                            | vn_core::ast::ExprKind::Identifier { .. }
                            | vn_core::ast::ExprKind::Missing
                            | vn_core::ast::ExprKind::This
                            | vn_core::ast::ExprKind::ImportMeta(_)
                            | vn_core::ast::ExprKind::Super
                            | vn_core::ast::ExprKind::Array { .. }
                            | vn_core::ast::ExprKind::Object { .. }
                            | vn_core::ast::ExprKind::Tuple { .. }
                            | vn_core::ast::ExprKind::Unary { .. }
                            | vn_core::ast::ExprKind::Update { .. }
                            | vn_core::ast::ExprKind::Binary { .. }
                            | vn_core::ast::ExprKind::Logical { .. }
                            | vn_core::ast::ExprKind::Assign { .. }
                            | vn_core::ast::ExprKind::Conditional { .. }
                            | vn_core::ast::ExprKind::Member { .. }
                            | vn_core::ast::ExprKind::Call { .. }
                            | vn_core::ast::ExprKind::New { .. }
                            | vn_core::ast::ExprKind::Sequence { .. }
                            | vn_core::ast::ExprKind::Paren { .. }
                            | vn_core::ast::ExprKind::Await { .. }
                            | vn_core::ast::ExprKind::Spawn { .. }
                            | vn_core::ast::ExprKind::Yield { .. }
                            | vn_core::ast::ExprKind::Spread { .. }
                            | vn_core::ast::ExprKind::Pipeline { .. }
                            | vn_core::ast::ExprKind::Range { .. }
                            | vn_core::ast::ExprKind::NonNull { .. }
                            | vn_core::ast::ExprKind::Try { .. }
                            | vn_core::ast::ExprKind::As { .. }
                            | vn_core::ast::ExprKind::Satisfies { .. }
                            | vn_core::ast::ExprKind::Match { .. }
                            | vn_core::ast::ExprKind::Is { .. }
                            | vn_core::ast::ExprKind::With { .. }
                            | vn_core::ast::ExprKind::MetaAccess { .. } => {}
                        },
                        vn_core::ast::StmtKind::Block { .. }
                        | vn_core::ast::StmtKind::Empty
                        | vn_core::ast::StmtKind::Expr { .. }
                        | vn_core::ast::StmtKind::Decl(_)
                        | vn_core::ast::StmtKind::Error
                        | vn_core::ast::StmtKind::If { .. }
                        | vn_core::ast::StmtKind::While { .. }
                        | vn_core::ast::StmtKind::DoWhile { .. }
                        | vn_core::ast::StmtKind::For { .. }
                        | vn_core::ast::StmtKind::ForIn { .. }
                        | vn_core::ast::StmtKind::ForOf { .. }
                        | vn_core::ast::StmtKind::Return { .. }
                        | vn_core::ast::StmtKind::Break { .. }
                        | vn_core::ast::StmtKind::Continue { .. }
                        | vn_core::ast::StmtKind::Throw { .. }
                        | vn_core::ast::StmtKind::Try { .. }
                        | vn_core::ast::StmtKind::Using { .. }
                        | vn_core::ast::StmtKind::Labeled { .. }
                        | vn_core::ast::StmtKind::Debugger => {
                            reason = Some("bodies that are not a single `return`")
                        }
                    }
                }
                vn_core::ast::StmtKind::Block { .. }
                | vn_core::ast::StmtKind::Empty
                | vn_core::ast::StmtKind::Expr { .. }
                | vn_core::ast::StmtKind::Decl(_)
                | vn_core::ast::StmtKind::Error
                | vn_core::ast::StmtKind::If { .. }
                | vn_core::ast::StmtKind::While { .. }
                | vn_core::ast::StmtKind::DoWhile { .. }
                | vn_core::ast::StmtKind::For { .. }
                | vn_core::ast::StmtKind::ForIn { .. }
                | vn_core::ast::StmtKind::ForOf { .. }
                | vn_core::ast::StmtKind::Return { .. }
                | vn_core::ast::StmtKind::Break { .. }
                | vn_core::ast::StmtKind::Continue { .. }
                | vn_core::ast::StmtKind::Throw { .. }
                | vn_core::ast::StmtKind::Try { .. }
                | vn_core::ast::StmtKind::Using { .. }
                | vn_core::ast::StmtKind::Labeled { .. }
                | vn_core::ast::StmtKind::Debugger => {
                    reason = Some("bodies that are not a single `return`")
                }
            }
        }
        if let Some(what) = reason {
            self.emit(
                vn_core::diagnostics::Diagnostic::error(
                    vn_core::diagnostics::ErrorCode::InvalidDecoratorSignature,
                    format!("`@inline` cannot apply to {what} (function '{name}')"),
                )
                .with_range(f.range),
            );
        }
    }
}
