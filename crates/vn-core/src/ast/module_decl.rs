use super::arena::ExprId;
use super::decl::{ClassDecl, Decl, FunctionDecl};
use super::expr::AstId;
use crate::source::SourceRange;
use crate::Atom;

#[derive(Clone, Debug)]
pub struct ImportDecl {
    pub ast_id: AstId,
    pub specifiers: Vec<ImportSpecifier>,
    pub source: Atom,
    pub is_type: bool,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub enum ImportSpecifier {
    Named {
        local: Atom,
        imported: Atom,
        range: SourceRange,
    },
    Default {
        local: Atom,
        range: SourceRange,
    },
    Namespace {
        local: Atom,
        range: SourceRange,
    },
}

impl ImportSpecifier {
    pub fn range(&self) -> &SourceRange {
        match self {
            ImportSpecifier::Named { range, .. } => range,
            ImportSpecifier::Default { range, .. } => range,
            ImportSpecifier::Namespace { range, .. } => range,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ExportDecl {
    Named {
        ast_id: AstId,
        specifiers: Vec<ExportSpecifier>,
        source: Option<Atom>,
        range: SourceRange,
    },
    Default {
        ast_id: AstId,
        declaration: Box<ExportDefaultDecl>,
        range: SourceRange,
    },
    All {
        ast_id: AstId,
        source: Atom,
        alias: Option<Atom>,
        range: SourceRange,
    },
    Decl {
        ast_id: AstId,
        declaration: Box<Decl>,
        range: SourceRange,
    },
}

impl ExportDecl {
    pub fn id(&self) -> AstId {
        match self {
            ExportDecl::Named { ast_id, .. } => *ast_id,
            ExportDecl::Default { ast_id, .. } => *ast_id,
            ExportDecl::All { ast_id, .. } => *ast_id,
            ExportDecl::Decl { ast_id, .. } => *ast_id,
        }
    }

    pub fn range(&self) -> &SourceRange {
        match self {
            ExportDecl::Named { range, .. } => range,
            ExportDecl::Default { range, .. } => range,
            ExportDecl::All { range, .. } => range,
            ExportDecl::Decl { range, .. } => range,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExportSpecifier {
    pub local: Atom,
    pub exported: Atom,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub enum ExportDefaultDecl {
    Function(FunctionDecl),
    Class(ClassDecl),
    Expr(ExprId),
}
