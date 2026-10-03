use varn_core::ast::{AstArena, Decl, ExportDecl, FunctionDecl};

pub(super) fn is_value_symbol(kind: crate::symbol::SymbolKind) -> bool {
    use crate::symbol::SymbolKind as K;
    matches!(
        kind,
        K::Var | K::Let | K::Const | K::Function | K::Class | K::Enum | K::Namespace | K::Struct
    )
}

pub(super) fn free_function(decl: &Decl) -> Option<&FunctionDecl> {
    match decl {
        Decl::Function(f) => Some(f),
        Decl::Export(ExportDecl::Decl { declaration, .. }) => match declaration.as_ref() {
            Decl::Function(f) => Some(f),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn namespace_decl(decl: &Decl) -> Option<&varn_core::ast::NamespaceDecl> {
    match decl {
        Decl::Namespace(n) => Some(n),
        Decl::Export(ExportDecl::Decl { declaration, .. }) => match declaration.as_ref() {
            Decl::Namespace(n) => Some(n),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn variable_decl(decl: &Decl) -> Option<&varn_core::ast::VariableDecl> {
    match decl {
        Decl::Variable(v) => Some(v),
        Decl::Export(ExportDecl::Decl { declaration, .. }) => match declaration.as_ref() {
            Decl::Variable(v) => Some(v),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn class_decl(decl: &Decl) -> Option<&varn_core::ast::ClassDecl> {
    match decl {
        Decl::Class(c) => Some(c),
        Decl::Export(ExportDecl::Decl { declaration, .. }) => match declaration.as_ref() {
            Decl::Class(c) => Some(c),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn enum_decl(decl: &Decl) -> Option<&varn_core::ast::EnumDecl> {
    match decl {
        Decl::Enum(e) => Some(e),
        Decl::Export(ExportDecl::Decl { declaration, .. }) => match declaration.as_ref() {
            Decl::Enum(e) => Some(e),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn anon_class_of<'a>(
    decl: &'a Decl,
    ast_arena: &'a AstArena,
) -> Option<&'a varn_core::ast::ClassDecl> {
    let v = variable_decl(decl)?;
    for d in &v.declarators {
        if let Some(init) = d.init {
            if let varn_core::ast::ExprKind::ClassExpr { declaration } = &ast_arena.expr(init).kind
            {
                return Some(declaration);
            }
        }
    }
    None
}
