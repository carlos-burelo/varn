use super::expr::{AstId, Expr};
use super::operators::Modifiers;
use super::pattern::Param;
use super::stmt::{Stmt, VariableDecl};
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

use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct SumTypeDecl {
    pub id: Rc<str>,
    pub ast_id: AstId,
    pub type_params: Vec<TypeParam>,
    pub variants: Vec<SumVariant>,
    pub doc: Option<String>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct SumVariant {
    pub name: Rc<str>,
    pub fields: Vec<SumField>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct SumField {
    pub name: Rc<str>,
    pub ty: TypeNode,
}

#[derive(Clone, Debug)]
pub struct FunctionDecl {
    pub id: Rc<str>,
    pub ast_id: AstId,

    pub id_offset: u32,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeNode>,
    pub body: Stmt,
    pub modifiers: Modifiers,
    pub decorators: Vec<Decorator>,
    pub doc: Option<String>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct ClassDecl {
    pub id: Option<Rc<str>>,
    pub ast_id: AstId,

    pub id_offset: u32,
    pub type_params: Vec<TypeParam>,
    pub super_class: Option<Expr>,
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
        body: Stmt,
        range: SourceRange,
    },
    Destructor {
        body: Stmt,
        range: SourceRange,
    },
    Method {
        key: Rc<str>,
        type_params: Vec<TypeParam>,
        params: Vec<Param>,
        return_type: Option<TypeNode>,
        body: Option<Stmt>,
        modifiers: Modifiers,
        decorators: Vec<Decorator>,
        range: SourceRange,
    },
    Property {
        key: Rc<str>,
        type_ann: Option<TypeNode>,
        init: Option<Expr>,
        modifiers: Modifiers,
        decorators: Vec<Decorator>,
        range: SourceRange,
    },
    Getter {
        key: Rc<str>,
        return_type: Option<TypeNode>,
        body: Option<Stmt>,
        modifiers: Modifiers,
        range: SourceRange,
    },
    Setter {
        key: Rc<str>,
        param: Param,
        body: Option<Stmt>,
        modifiers: Modifiers,
        range: SourceRange,
    },
    StaticBlock {
        body: Stmt,
        range: SourceRange,
    },
}

#[derive(Clone, Debug)]
pub struct InterfaceDecl {
    pub id: Rc<str>,
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
        key: Rc<str>,
        type_ann: TypeNode,
        optional: bool,
        readonly: bool,
        range: SourceRange,
    },
    Method {
        key: Rc<str>,
        type_params: Vec<TypeParam>,
        params: Vec<Param>,
        return_type: Option<TypeNode>,
        optional: bool,
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
    pub id: Rc<str>,
    pub ast_id: AstId,
    pub type_params: Vec<TypeParam>,
    pub alias: TypeNode,
    pub doc: Option<String>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct EnumDecl {
    pub id: Rc<str>,
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
    pub id: Rc<str>,
    pub init: Option<Expr>,

    pub payload_fields: Vec<EnumField>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct EnumField {
    pub name: Rc<str>,
    pub ty: TypeNode,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct NamespaceDecl {
    pub id: Rc<str>,
    pub ast_id: AstId,
    pub body: Vec<Decl>,
    pub doc: Option<String>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct ImportDecl {
    pub ast_id: AstId,
    pub specifiers: Vec<ImportSpecifier>,
    pub source: Rc<str>,
    pub is_type: bool,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub enum ImportSpecifier {
    Named {
        local: Rc<str>,
        imported: Rc<str>,
        range: SourceRange,
    },
    Default {
        local: Rc<str>,
        range: SourceRange,
    },
    Namespace {
        local: Rc<str>,
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
        source: Option<Rc<str>>,
        range: SourceRange,
    },
    Default {
        ast_id: AstId,
        declaration: Box<ExportDefaultDecl>,
        range: SourceRange,
    },
    All {
        ast_id: AstId,
        source: Rc<str>,
        alias: Option<Rc<str>>,
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
    pub local: Rc<str>,
    pub exported: Rc<str>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub enum ExportDefaultDecl {
    Function(FunctionDecl),
    Class(ClassDecl),
    Expr(Expr),
}

#[derive(Clone, Debug)]
pub struct ExtensionDecl {
    pub id: Option<Rc<str>>,
    pub ast_id: AstId,
    pub target: TypeNode,
    pub members: Vec<ExtensionMember>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub enum ExtensionMember {
    Method(FunctionDecl),
    Getter {
        key: Rc<str>,
        return_type: Option<TypeNode>,
        body: Stmt,
        modifiers: Modifiers,
        range: SourceRange,
    },
    Setter {
        key: Rc<str>,
        param: Param,
        body: Stmt,
        modifiers: Modifiers,
        range: SourceRange,
    },
}

#[derive(Clone, Debug)]
pub struct StructDecl {
    pub id: Rc<str>,
    pub ast_id: AstId,
    pub fields: Vec<StructField>,
    pub doc: Option<String>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct StructField {
    pub name: Rc<str>,
    pub type_ann: TypeNode,
    pub default: Option<Expr>,
    pub range: SourceRange,
}
