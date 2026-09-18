use crate::ast::expr::ExprKind;
use crate::ast::stmt::StmtKind;
use crate::source::SourceRange;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExprId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StmtId(u32);

impl ExprId {
    /// The node's position in the arena. Doubles as its `AstId` (the checker
    /// keys `CheckResult::expr_table` by this raw index, not by a separate
    /// stable id): every expression is minted once, from `alloc_expr`, so the
    /// arena position already IS a per-node identity.
    pub fn index(&self) -> u32 {
        self.0
    }
}

impl StmtId {
    /// See [`ExprId::index`].
    pub fn index(&self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug)]
pub struct ExprNode {
    pub range: SourceRange,
    pub kind: ExprKind,
}

#[derive(Clone, Debug)]
pub struct StmtNode {
    pub range: SourceRange,
    pub kind: StmtKind,
}

#[derive(Debug, Default)]
pub struct AstArena {
    exprs: Vec<ExprNode>,
    stmts: Vec<StmtNode>,
}

impl AstArena {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn alloc_expr(&mut self, kind: ExprKind, range: SourceRange) -> ExprId {
        let id = self.exprs.len() as u32;
        self.exprs.push(ExprNode { range, kind });
        ExprId(id)
    }

    pub fn alloc_stmt(&mut self, kind: StmtKind, range: SourceRange) -> StmtId {
        let id = self.stmts.len() as u32;
        self.stmts.push(StmtNode { range, kind });
        StmtId(id)
    }

    pub fn expr(&self, id: ExprId) -> &ExprNode {
        &self.exprs[id.0 as usize]
    }

    pub fn expr_mut(&mut self, id: ExprId) -> &mut ExprNode {
        &mut self.exprs[id.0 as usize]
    }

    pub fn stmt(&self, id: StmtId) -> &StmtNode {
        &self.stmts[id.0 as usize]
    }

    pub fn stmt_mut(&mut self, id: StmtId) -> &mut StmtNode {
        &mut self.stmts[id.0 as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::expr::ExprKind;
    use crate::source::SourceRange;

    #[test]
    fn alloc_and_get_roundtrips() {
        let mut arena = AstArena::new();
        let id = arena.alloc_expr(ExprKind::NullLiteral, SourceRange::default());
        assert!(matches!(arena.expr(id).kind, ExprKind::NullLiteral));
    }

    #[test]
    fn distinct_allocations_get_distinct_ids() {
        let mut arena = AstArena::new();
        let a = arena.alloc_expr(ExprKind::NullLiteral, SourceRange::default());
        let b = arena.alloc_expr(ExprKind::This, SourceRange::default());
        assert_ne!(a, b);
    }
}
