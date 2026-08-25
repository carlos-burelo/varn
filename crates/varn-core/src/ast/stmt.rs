use super::decl::Decl;
use super::expr::{AstId, Expr};
use super::operators::VarKind;
use super::pattern::Pattern;
use super::types::TypeNode;
use crate::source::SourceRange;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct VarDeclarator {
    pub id: Pattern,
    pub type_ann: Option<TypeNode>,
    pub init: Option<Expr>,
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
pub struct Stmt {
    pub id: AstId,
    pub range: SourceRange,
    pub kind: StmtKind,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Block {
        stmts: Vec<Stmt>,
    },
    Empty,
    Expr {
        expression: Box<Expr>,
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
        test: Box<Expr>,
        consequent: Box<Stmt>,
        alternate: Option<Box<Stmt>>,
    },
    While {
        test: Box<Expr>,
        body: Box<Stmt>,
    },
    DoWhile {
        body: Box<Stmt>,
        test: Box<Expr>,
    },
    For {
        init: Option<Box<ForInit>>,
        test: Option<Box<Expr>>,
        update: Option<Box<Expr>>,
        body: Box<Stmt>,
    },
    ForIn {
        kind: VarKind,
        left: Pattern,
        right: Box<Expr>,
        body: Box<Stmt>,
    },
    ForOf {
        kind: VarKind,
        left: Pattern,
        right: Box<Expr>,
        body: Box<Stmt>,
        is_await: bool,
    },
    Switch {
        discriminant: Box<Expr>,
        cases: Vec<SwitchCase>,
    },

    Return {
        argument: Option<Box<Expr>>,
    },
    Break {
        label: Option<Rc<str>>,
    },
    Continue {
        label: Option<Rc<str>>,
    },
    Throw {
        argument: Box<Expr>,
    },
    Try {
        block: Box<Stmt>,
        catch: Option<Box<CatchClause>>,
        finally: Option<Box<Stmt>>,
    },
    Using {
        declarations: Vec<VarDeclarator>,
        is_await: bool,
    },
    Labeled {
        label: Rc<str>,
        body: Box<Stmt>,
    },
    Debugger,
}

impl Stmt {
    pub fn new(id: AstId, range: SourceRange, kind: StmtKind) -> Self {
        Self { id, range, kind }
    }

    pub fn new_with_range(range: SourceRange, kind: StmtKind) -> Self {
        Self { id: 0, range, kind }
    }

    pub fn id(&self) -> AstId {
        match &self.kind {
            StmtKind::Decl(d) => d.id(),
            _ => self.id,
        }
    }

    pub fn range(&self) -> &SourceRange {
        match &self.kind {
            StmtKind::Decl(d) => d.range(),
            _ => &self.range,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ForInit {
    Var {
        kind: VarKind,
        declarators: Vec<VarDeclarator>,
    },
    Expr(Expr),
}

#[derive(Clone, Debug)]
pub struct SwitchCase {
    pub test: Option<Expr>,
    pub body: Vec<Stmt>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct CatchClause {
    pub param: Option<Pattern>,
    pub body: Box<Stmt>,
    pub range: SourceRange,
}
