use std::sync::Arc;
use varn_core::ast::operators::BinaryOp;
use varn_core::ast::{AstArena, ExprId, Pattern, PropKey, StmtId, StmtKind};
use varn_core::AtomInterner;
use varn_tir::{BackendTy, DynReason, Resolution, Span, TirBinOp, TirExpr, TirExprKind};

pub(super) fn pattern_lead(p: &Pattern, interner: &AtomInterner) -> Arc<str> {
    match p {
        Pattern::Identifier { name, .. } => Arc::from(interner.resolve(*name)),
        Pattern::Array { .. } | Pattern::Object { .. } | Pattern::Assignment { .. } | Pattern::Rest { .. } => Arc::from("_"),
    }
}

pub(super) fn span_of(ast_arena: &AstArena, e: ExprId) -> Span {
    let range = ast_arena.expr(e).range;
    Span {
        start: range.start.offset,
        end: range.end.offset,
    }
}

pub(super) fn placeholder(reason: DynReason) -> TirExpr {
    TirExpr {
        kind: TirExprKind::NullLit,
        ty: BackendTy::Dynamic(reason),
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
    op: varn_core::ast::operators::AssignOp,
) -> Result<Option<TirBinOp>, ()> {
    use varn_core::ast::operators::AssignOp as A;
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
        BinaryOp::Instanceof | BinaryOp::In => return None,
    })
}

pub(super) fn has_continue(ast_arena: &AstArena, stmt: StmtId) -> bool {
    fn check(ast_arena: &AstArena, stmt: StmtId, in_nested_loop: bool) -> bool {
        match &ast_arena.stmt(stmt).kind {
            StmtKind::Continue { label } => {
                if in_nested_loop {
                    label.is_some()
                } else {
                    true
                }
            }
            StmtKind::Block { stmts } => stmts.iter().any(|&s| check(ast_arena, s, in_nested_loop)),
            StmtKind::If {
                consequent,
                alternate,
                ..
            } => {
                check(ast_arena, *consequent, in_nested_loop)
                    || alternate.is_some_and(|a| check(ast_arena, a, in_nested_loop))
            }
            StmtKind::Switch { cases, .. } => cases
                .iter()
                .any(|c| c.body.iter().any(|&s| check(ast_arena, s, in_nested_loop))),
            StmtKind::Try {
                block,
                catches,
                finally,
            } => {
                check(ast_arena, *block, in_nested_loop)
                    || catches
                        .iter()
                        .any(|c| check(ast_arena, c.body, in_nested_loop))
                    || finally.is_some_and(|f| check(ast_arena, f, in_nested_loop))
            }
            StmtKind::Labeled { body, .. } => check(ast_arena, *body, in_nested_loop),
            StmtKind::While { body, .. }
            | StmtKind::DoWhile { body, .. }
            | StmtKind::For { body, .. }
            | StmtKind::ForIn { body, .. }
            | StmtKind::ForOf { body, .. } => check(ast_arena, *body, true),
            StmtKind::Empty | StmtKind::Expr { .. } | StmtKind::Decl(_) | StmtKind::Error | StmtKind::Return { .. } | StmtKind::Break { .. } | StmtKind::Throw { .. } | StmtKind::Using { .. } | StmtKind::Debugger => false,
        }
    }
    check(ast_arena, stmt, false)
}
