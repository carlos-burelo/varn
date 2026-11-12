use std::sync::Arc;
use vn_core::ast::operators::BinaryOp;
use vn_core::ast::{AstArena, ExprId, ExprKind, MatchBody, Pattern, PropKey, StmtId, StmtKind};
use vn_core::AtomInterner;
use vn_tir::{BackendTy, Resolution, Span, TirBinOp, TirExpr, TirExprKind};

pub(super) fn pattern_lead(p: &Pattern, interner: &AtomInterner) -> Arc<str> {
    match p {
        Pattern::Identifier { name, .. } => Arc::from(interner.resolve(*name)),
        Pattern::Array { .. }
        | Pattern::Tuple { .. }
        | Pattern::Object { .. }
        | Pattern::Assignment { .. }
        | Pattern::Rest { .. } => Arc::from(vn_core::TokenKind::Placeholder.as_str()),
    }
}

pub(super) fn span_of(ast_arena: &AstArena, e: ExprId) -> Span {
    let range = ast_arena.expr(e).range;
    Span {
        start: range.start.offset,
        end: range.end.offset,
    }
}

pub(super) fn placeholder() -> TirExpr {
    TirExpr {
        kind: TirExprKind::NullLit,
        ty: crate::ty::NULL_TY,
        res: Resolution::None,
        span: Span::EMPTY,
    }
}

pub(super) fn prop_key_name(key: &PropKey) -> Option<Arc<str>> {
    match key {
        PropKey::Identifier(s) | PropKey::Str(s) => Some(Arc::from(s.as_str())),
        PropKey::Int(n) => Some(Arc::from(n.to_string())),
        PropKey::Computed(_) => None,
    }
}

pub(super) fn bool_lit(v: bool) -> TirExpr {
    TirExpr {
        kind: TirExprKind::BoolLit(v),
        ty: BackendTy::Bool,
        res: Resolution::None,
        span: Span::EMPTY,
    }
}

pub(super) fn assign_bin_op(
    op: vn_core::ast::operators::AssignOp,
) -> Result<Option<TirBinOp>, ()> {
    use vn_core::ast::operators::AssignOp as A;
    Ok(Some(match op {
        A::Assign => return Ok(None),
        A::AddAssign => TirBinOp::Add,
        A::SubAssign => TirBinOp::Sub,
        A::MulAssign => TirBinOp::Mul,
        A::DivAssign => TirBinOp::Div,
        A::ModAssign => TirBinOp::Mod,
        A::PowAssign => TirBinOp::Pow,
        A::BitAndAssign => TirBinOp::BitAnd,
        A::BitOrAssign => TirBinOp::BitOr,
        A::BitXorAssign => TirBinOp::BitXor,
        A::ShlAssign => TirBinOp::Shl,
        A::ShrAssign => TirBinOp::Shr,
        A::UShrAssign => TirBinOp::Ushr,
        A::AndAssign | A::OrAssign | A::NullishAssign => return Err(()),
    }))
}

pub(super) fn int_lit(v: i64) -> TirExpr {
    TirExpr {
        kind: TirExprKind::IntLit(v),
        ty: BackendTy::Int,
        res: Resolution::None,
        span: Span::EMPTY,
    }
}

pub(super) fn bin_op(op: BinaryOp) -> Option<TirBinOp> {
    Some(match op {
        BinaryOp::Add => TirBinOp::Add,
        BinaryOp::Sub => TirBinOp::Sub,
        BinaryOp::Mul => TirBinOp::Mul,
        BinaryOp::Div => TirBinOp::Div,
        BinaryOp::Mod => TirBinOp::Mod,
        BinaryOp::Pow => TirBinOp::Pow,
        BinaryOp::Eq => TirBinOp::Eq,
        BinaryOp::NotEq => TirBinOp::Ne,
        BinaryOp::Lt => TirBinOp::Lt,
        BinaryOp::Gt => TirBinOp::Gt,
        BinaryOp::LtEq => TirBinOp::Le,
        BinaryOp::GtEq => TirBinOp::Ge,
        BinaryOp::BitAnd => TirBinOp::BitAnd,
        BinaryOp::BitOr => TirBinOp::BitOr,
        BinaryOp::BitXor => TirBinOp::BitXor,
        BinaryOp::Shl => TirBinOp::Shl,
        BinaryOp::Shr => TirBinOp::Shr,
        BinaryOp::UShr => TirBinOp::Ushr,
        BinaryOp::In => return None,
    })
}

pub(super) fn has_continue(ast_arena: &AstArena, stmt: StmtId) -> bool {
    has_continue_in(ast_arena, stmt, false)
}

fn has_continue_in(ast_arena: &AstArena, stmt: StmtId, in_nested_loop: bool) -> bool {
    match &ast_arena.stmt(stmt).kind {
        StmtKind::Continue { label } => {
            if in_nested_loop {
                label.is_some()
            } else {
                true
            }
        }
        StmtKind::Block { stmts } => stmts
            .iter()
            .any(|&s| has_continue_in(ast_arena, s, in_nested_loop)),
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => {
            has_continue_in(ast_arena, *consequent, in_nested_loop)
                || alternate.is_some_and(|a| has_continue_in(ast_arena, a, in_nested_loop))
        }
        StmtKind::Try {
            block,
            catches,
            finally,
        } => {
            has_continue_in(ast_arena, *block, in_nested_loop)
                || catches
                    .iter()
                    .any(|c| has_continue_in(ast_arena, c.body, in_nested_loop))
                || finally.is_some_and(|f| has_continue_in(ast_arena, f, in_nested_loop))
        }
        StmtKind::Labeled { body, .. } => has_continue_in(ast_arena, *body, in_nested_loop),
        StmtKind::While { body, .. }
        | StmtKind::DoWhile { body, .. }
        | StmtKind::For { body, .. }
        | StmtKind::ForIn { body, .. }
        | StmtKind::ForOf { body, .. } => has_continue_in(ast_arena, *body, true),
        StmtKind::Expr { expression } => {
            expression_has_continue(ast_arena, *expression, in_nested_loop)
        }
        StmtKind::Empty
        | StmtKind::Decl(_)
        | StmtKind::Error
        | StmtKind::Return { .. }
        | StmtKind::Break { .. }
        | StmtKind::Throw { .. }
        | StmtKind::Using { .. }
        | StmtKind::Debugger => false,
    }
}

fn expression_has_continue(ast_arena: &AstArena, expr: ExprId, in_nested_loop: bool) -> bool {
    let kind = &ast_arena.expr(expr).kind;
    if let ExprKind::Paren { expression } = kind {
        return expression_has_continue(ast_arena, *expression, in_nested_loop);
    }
    if let ExprKind::Match { cases, .. } = kind {
        return cases.iter().any(|case| match &case.body {
            MatchBody::Block(stmt) => has_continue_in(ast_arena, *stmt, in_nested_loop),
            MatchBody::Expr(expression) => {
                expression_has_continue(ast_arena, *expression, in_nested_loop)
            }
        });
    }
    false
}
