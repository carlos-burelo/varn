use super::arena::{ExprId, StmtId};
use super::decl::Decl;
use super::expr::AstId;
use super::operators::VarKind;
use super::pattern::Pattern;
use super::types::TypeNode;
use crate::source::SourceRange;
use crate::Atom;

#[derive(Clone, Debug)]
pub struct VarDeclarator {
    pub id: Pattern,
    pub type_ann: Option<TypeNode>,
    pub init: Option<ExprId>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct VariableDecl {
    pub kind: VarKind,
    pub ast_id: AstId,
    pub declarators: Vec<VarDeclarator>,
    pub is_declare: bool,
    pub doc: Option<String>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Block {
        stmts: Vec<StmtId>,
    },
    Empty,
    Expr {
        expression: ExprId,
    },
    Decl(Box<Decl>),

    /// A span the parser could not parse, preserved rather than discarded.
    ///
    /// `Stmt::range` covers the recovered text, so consumers that want the
    /// tokens re-slice the token stream by range — the tokens are not copied
    /// into the tree. Keeping the node (instead of dropping the statement, as
    /// recovery used to) is what guarantees every byte of source stays
    /// reachable from the tree.
    ///
    /// Checker: ignored, no diagnostic (the parser already reported one).
    /// Compiler: hard error before lowering.
    Error,

    If {
        test: ExprId,
        consequent: StmtId,
        alternate: Option<StmtId>,
    },
    While {
        test: ExprId,
        body: StmtId,
    },
    DoWhile {
        body: StmtId,
        test: ExprId,
    },
    For {
        init: Option<Box<ForInit>>,
        test: Option<ExprId>,
        update: Option<ExprId>,
        body: StmtId,
    },
    ForIn {
        kind: VarKind,
        left: Pattern,
        right: ExprId,
        body: StmtId,
    },
    ForOf {
        kind: VarKind,
        left: Pattern,
        right: ExprId,
        body: StmtId,
        is_await: bool,
    },
    Switch {
        discriminant: ExprId,
        cases: Vec<SwitchCase>,
    },

    Return {
        argument: Option<ExprId>,
    },
    Break {
        label: Option<Atom>,
    },
    Continue {
        label: Option<Atom>,
    },
    Throw {
        argument: ExprId,
    },
    Try {
        block: StmtId,
        catches: Vec<CatchClause>,
        finally: Option<StmtId>,
    },
    Using {
        declarations: Vec<VarDeclarator>,
        is_await: bool,
    },
    Labeled {
        label: Atom,
        body: StmtId,
    },
    Debugger,
}

#[derive(Clone, Debug)]
pub enum ForInit {
    Var {
        kind: VarKind,
        declarators: Vec<VarDeclarator>,
    },
    Expr(ExprId),
}

#[derive(Clone, Debug)]
pub struct SwitchCase {
    pub test: Option<ExprId>,
    pub body: Vec<StmtId>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct CatchClause {
    pub param: Option<Pattern>,
    pub type_ann: Option<TypeNode>,
    pub body: StmtId,
    pub range: SourceRange,
}
