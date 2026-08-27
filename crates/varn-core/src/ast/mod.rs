pub mod decl;
pub mod expr;
pub mod operators;
pub mod pattern;
pub mod program;
pub mod stmt;
pub mod types;

pub use program::Program;

pub use decl::{
    ClassDecl, ClassMember, Decl, EnumDecl, EnumField, EnumMember, ExportDecl, ExportDefaultDecl,
    ExtensionDecl, ExtensionMember, FunctionDecl, ImportDecl, ImportSpecifier, InterfaceDecl,
    InterfaceMember, NamespaceDecl, StructDecl, SumField, SumTypeDecl, SumVariant, TypeAliasDecl,
};
pub use expr::{
    Arg, ArrayEl, ArrowBody, AstId, Expr, ExprKind, MatchBody, MatchCase, ObjectProp, PropKey,
    TemplatePart,
};
pub use operators::{
    AssignOp, BinaryOp, LogicalOp, Modifiers, UnaryOp, UpdateOp, VarKind, Visibility,
};
pub use pattern::{ArrayPatternEl, MatchBinding, MatchPattern, ObjPatternProp, Param, Pattern};
pub use stmt::{CatchClause, ForInit, Stmt, StmtKind, SwitchCase, VarDeclarator, VariableDecl};
pub use types::{AstTypeKind, Decorator, TypeNode, TypeParam};

use crate::source::SourceRange;
use rustc_hash::FxHashMap;



#[derive(Default, Clone, Debug)]
pub struct AstMetadata {
    pub ranges: FxHashMap<AstId, SourceRange>,
}

impl AstMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, id: AstId, range: SourceRange) {
        self.ranges.insert(id, range);
    }

    pub fn get(&self, id: AstId) -> Option<SourceRange> {
        self.ranges.get(&id).copied()
    }
}
