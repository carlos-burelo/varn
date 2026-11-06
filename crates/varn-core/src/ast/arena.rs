use crate::ast::expr::ExprKind;
use crate::ast::stmt::StmtKind;
use crate::source::SourceRange;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExprId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StmtId(u32);

impl ExprId {
    pub fn index(&self) -> u32 {
        self.0
    }
}

impl StmtId {
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

    pub fn exprs(&self) -> impl Iterator<Item = &ExprNode> {
        self.exprs.iter()
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
