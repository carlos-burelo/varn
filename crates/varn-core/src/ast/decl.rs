use super::arena::{ExprId, StmtId};
use super::expr::AstId;
use super::operators::Modifiers;
use super::pattern::Param;
use super::stmt::VariableDecl;
use super::types::{Decorator, TypeNode, TypeParam};
use crate::source::SourceRange;

#[derive(Clone, Debug)]
pub enum Decl {
    Variable(VariableDecl),
    Function(FunctionDecl),
    Class(ClassDecl),
    Interface(InterfaceDecl),
    TypeAlias(TypeAliasDecl),
    Enum(EnumDecl),
    Namespace(NamespaceDecl),
    Import(ImportDecl),
    Export(ExportDecl),
    Extension(ExtensionDecl),
    Struct(StructDecl),
    SumType(SumTypeDecl),
}

impl Decl {
    pub fn id(&self) -> AstId {
        match self {
            Decl::Variable(d) => d.ast_id,
            Decl::Function(d) => d.ast_id,
            Decl::Class(d) => d.ast_id,
            Decl::Interface(d) => d.ast_id,
            Decl::TypeAlias(d) => d.ast_id,
            Decl::Enum(d) => d.ast_id,
            Decl::Namespace(d) => d.ast_id,
            Decl::Import(d) => d.ast_id,
            Decl::Export(d) => d.id(),
            Decl::Extension(d) => d.ast_id,
            Decl::Struct(d) => d.ast_id,
            Decl::SumType(d) => d.ast_id,
        }
    }

    pub fn range(&self) -> &SourceRange {
        match self {
            Decl::Variable(d) => &d.range,
            Decl::Function(d) => &d.range,
            Decl::Class(d) => &d.range,
            Decl::Interface(d) => &d.range,
            Decl::TypeAlias(d) => &d.range,
            Decl::Enum(d) => &d.range,
            Decl::Namespace(d) => &d.range,
            Decl::Import(d) => &d.range,
            Decl::Export(d) => d.range(),
            Decl::Extension(d) => &d.range,
            Decl::Struct(d) => &d.range,
            Decl::SumType(d) => &d.range,
        }
    }
}

use crate::Atom;

#[derive(Clone, Debug)]
pub struct SumTypeDecl {
    pub id: Atom,
    pub ast_id: AstId,
    pub type_params: Vec<TypeParam>,
    pub variants: Vec<SumVariant>,
    pub doc: Option<String>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct SumVariant {
    pub name: Atom,
    pub fields: Vec<SumField>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct SumField {
    pub name: Atom,
    pub ty: TypeNode,
}

#[derive(Clone, Debug)]
pub struct FunctionDecl {
    pub id: Atom,
    pub ast_id: AstId,

    pub id_offset: u32,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeNode>,
    pub body: StmtId,
    pub modifiers: Modifiers,
    pub decorators: Vec<Decorator>,
    pub doc: Option<String>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct ClassDecl {
    pub id: Option<Atom>,
    pub ast_id: AstId,

    pub id_offset: u32,
    pub type_params: Vec<TypeParam>,
    pub primary_params: Option<Vec<Param>>,
    pub super_class: Option<ExprId>,
    pub super_type_args: Vec<TypeNode>,
    pub implements: Vec<TypeNode>,
    pub body: Vec<ClassMember>,
    pub modifiers: Modifiers,
    pub decorators: Vec<Decorator>,
    pub doc: Option<String>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub enum ClassMember {
    Constructor {
        params: Vec<Param>,
        body: StmtId,
        range: SourceRange,
    },
    Destructor {
        body: StmtId,
        range: SourceRange,
    },
    Method {
        key: Atom,
        type_params: Vec<TypeParam>,
        params: Vec<Param>,
        return_type: Option<TypeNode>,
        body: Option<StmtId>,
        modifiers: Modifiers,
        decorators: Vec<Decorator>,
        range: SourceRange,
    },
    Property {
        key: Atom,
        type_ann: Option<TypeNode>,
        init: Option<ExprId>,
        modifiers: Modifiers,
        decorators: Vec<Decorator>,
        range: SourceRange,
    },
    Getter {
        key: Atom,
        return_type: Option<TypeNode>,
        body: Option<StmtId>,
        modifiers: Modifiers,
        range: SourceRange,
    },
    Setter {
        key: Atom,
        param: Param,
        body: Option<StmtId>,
        modifiers: Modifiers,
        range: SourceRange,
    },
    StaticBlock {
        body: StmtId,
        range: SourceRange,
    },
}

#[derive(Clone, Debug)]
pub struct InterfaceDecl {
    pub id: Atom,
    pub ast_id: AstId,
    pub type_params: Vec<TypeParam>,
    pub extends: Vec<TypeNode>,
    pub body: Vec<InterfaceMember>,
    pub doc: Option<String>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum InterfaceMember {
    Property {
        key: Atom,
        type_ann: TypeNode,
        optional: bool,
        readonly: bool,
        range: SourceRange,
    },
    Method {
        key: Atom,
        type_params: Vec<TypeParam>,
        params: Vec<Param>,
        return_type: Option<TypeNode>,
        optional: bool,
        /// An `async` signature declares an implementation that returns a
        /// `Task`, exactly as it does on a class. Without it, an interface
        /// could not describe any async API.
        is_async: bool,
        range: SourceRange,
    },
    Index {
        param: Param,
        return_type: TypeNode,
        range: SourceRange,
    },
    Callable {
        params: Vec<Param>,
        return_type: TypeNode,
        range: SourceRange,
    },
}

#[derive(Clone, Debug)]
pub struct TypeAliasDecl {
    pub id: Atom,
    pub ast_id: AstId,
    pub type_params: Vec<TypeParam>,
    pub alias: TypeNode,
    pub doc: Option<String>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct EnumDecl {
    pub id: Atom,
    pub ast_id: AstId,
    pub type_params: Vec<TypeParam>,
    pub implements: Vec<TypeNode>,
    pub members: Vec<EnumMember>,
    pub body: Vec<ClassMember>,
    pub doc: Option<String>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct EnumMember {
    pub id: Atom,
    pub init: Option<ExprId>,

    pub payload_fields: Vec<EnumField>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct EnumField {
    pub name: Atom,
    pub ty: TypeNode,
    pub init: Option<ExprId>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct NamespaceDecl {
    pub id: Atom,
    pub ast_id: AstId,
    pub body: Vec<Decl>,
    pub doc: Option<String>,
    pub range: SourceRange,
}

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

#[derive(Clone, Debug)]
pub struct ExtensionDecl {
    pub id: Option<Atom>,
    pub ast_id: AstId,
    pub target: TypeNode,
    pub members: Vec<ExtensionMember>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub enum ExtensionMember {
    Method(FunctionDecl),
    Getter {
        key: Atom,
        return_type: Option<TypeNode>,
        body: StmtId,
        modifiers: Modifiers,
        range: SourceRange,
    },
    Setter {
        key: Atom,
        param: Param,
        body: StmtId,
        modifiers: Modifiers,
        range: SourceRange,
    },
}

#[derive(Clone, Debug)]
pub struct StructDecl {
    pub id: Atom,
    pub ast_id: AstId,
    pub fields: Vec<StructField>,
    pub doc: Option<String>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct StructField {
    pub name: Atom,
    pub type_ann: TypeNode,
    pub default: Option<ExprId>,
    pub range: SourceRange,
}
