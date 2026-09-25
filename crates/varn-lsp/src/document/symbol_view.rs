//! A checker symbol, viewed as the editor needs it.

use varn_checker::{SymbolKind, Type};

use super::SemanticDB;

/// A symbol the checker bound, viewed as the editor needs it.
///
/// Borrows `varn_checker::Symbol` — it does not copy it. What used to sit here
/// was `SymbolView<'_>`: the same twenty fields *materialized* for every symbol on
/// every keystroke, with the signature pre-flattened into `String`s and the type
/// cloned. Everything below the first two fields is derived on demand, so the
/// editor always reports what the checker currently holds.
#[derive(Clone, Copy)]
pub struct SymbolView<'a> {
    pub id: varn_checker::SymbolId,
    pub sym: &'a varn_checker::symbol::Symbol,
    pub(super) uri: &'a str,
    pub(super) ty: &'a Type,
    /// The atom and type tables the symbol's names and type index into.
    pub(super) db: &'a SemanticDB,
}

/// `dynamic`, for symbols the checker left untyped.
pub(super) static DYNAMIC_TY: Type = Type(varn_checker::types::CheckerTyId::DYNAMIC, false);

impl std::fmt::Debug for SymbolView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SymbolView")
            .field("name", &self.name())
            .field("kind", &self.kind())
            .finish()
    }
}

impl<'a> SymbolView<'a> {
    fn text(&self, atom: varn_core::Atom) -> &'a str {
        self.db.bind.interner.resolve(atom)
    }
    pub fn name(&self) -> &'a str {
        self.text(self.sym.name)
    }
    pub fn kind(&self) -> SymbolKind {
        self.sym.kind
    }
    pub fn ty(&self) -> &'a Type {
        self.ty
    }
    /// 0-based, as LSP positions are; the checker counts from 1.
    pub fn line(&self) -> u32 {
        self.sym.line.saturating_sub(1)
    }
    pub fn col(&self) -> u32 {
        self.sym.col
    }
    pub fn end_line(&self) -> u32 {
        if self.sym.full_range.end.line > 0 {
            self.sym.full_range.end.line.saturating_sub(1)
        } else {
            self.line()
        }
    }
    pub fn end_col(&self) -> u32 {
        if self.sym.full_range.end.line > 0 {
            self.sym.full_range.end.column
        } else {
            self.sym.col + self.name().chars().count() as u32
        }
    }
    pub fn full_range(&self) -> varn_core::SourceRange {
        self.sym.full_range
    }
    pub fn doc(&self) -> Option<&'a str> {
        self.sym.doc.map(|a| self.text(a))
    }
    pub fn origin(&self) -> Option<&'a str> {
        self.sym.origin_module.map(|a| self.text(a))
    }
    pub fn is_async(&self) -> bool {
        self.sym.is_async
    }
    pub fn is_generator(&self) -> bool {
        self.sym.is_generator
    }
    pub fn has_explicit_type(&self) -> bool {
        self.sym.has_explicit_type
    }
    pub fn type_params(&self) -> Vec<String> {
        self.sym
            .type_params
            .iter()
            .map(|a| self.text(*a).to_owned())
            .collect()
    }
    /// The function shape of this symbol's type, if it is a function.
    fn fn_shape(&self) -> Option<varn_checker::types::FunctionType> {
        self.db.fn_shape(self.ty)
    }
    pub fn is_arrow(&self) -> bool {
        self.fn_shape().is_some_and(|f| f.is_arrow)
    }
    pub fn is_from_stdlib(&self) -> bool {
        self.origin().is_some_and(|m| {
            m.starts_with("std:") || m.starts_with("core:") || m.starts_with("runtime:")
        })
    }
    /// A function reads as its return type; everything else as its own.
    pub fn type_str(&self) -> String {
        match (self.fn_shape(), self.kind()) {
            (Some(ft), SymbolKind::Function | SymbolKind::Method) => {
                self.db.ty_text(&Type(ft.return_type, false))
            }
            _ => self.db.ty_text(self.ty),
        }
    }
    pub fn params_str(&self) -> String {
        match self.fn_shape() {
            Some(ft) => ft
                .params
                .iter()
                .map(|p| {
                    format!(
                        "{}: {}{}",
                        p.name.as_deref().unwrap_or("_"),
                        self.db.ty_text(&Type(p.ty, false)),
                        if p.optional { "?" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join(", "),
            None => String::new(),
        }
    }
    pub fn global_key(&self, is_global: bool) -> String {
        crate::pipeline::stable_global_key(
            self.uri,
            self.name(),
            self.kind(),
            Some(self.id),
            self.origin(),
            self.sym.original_name.map(|a| self.text(a)),
            is_global,
        )
    }
}
