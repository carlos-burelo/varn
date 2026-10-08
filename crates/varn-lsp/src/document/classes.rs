use varn_core::ast::{ClassDecl, Decl, StmtKind};

use super::DocumentState;

pub fn top_level_classes(state: &DocumentState) -> impl Iterator<Item = &ClassDecl> {
    let body = state
        .ast
        .as_ref()
        .map(|p| p.body.as_slice())
        .unwrap_or_default();
    body.iter()
        .filter_map(|&id| match &state.ast_arena.stmt(id).kind {
            StmtKind::Decl(decl) => match decl.as_ref() {
                Decl::Class(c) => Some(c),
                _ => None,
            },
            _ => None,
        })
}

pub fn class_name(state: &DocumentState, class: &ClassDecl) -> Option<String> {
    class.id.map(|n| state.name(n).to_owned())
}

pub fn super_name(state: &DocumentState, class: &ClassDecl) -> Option<String> {
    let super_expr = class.super_class?;
    if let varn_core::ast::ExprKind::Identifier { name } = &state.ast_arena.expr(super_expr).kind {
        return Some(state.name(*name).to_owned());
    }
    None
}
