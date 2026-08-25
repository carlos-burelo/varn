mod chain_queries;
pub mod import;
mod resolution;
mod symbol_queries;

use rustc_hash::FxHashMap;
use std::collections::{HashMap, HashSet};

use varn_checker::{ScopeArena, SymbolArena, SymbolKind, Type};
use varn_core::TokenKind;

pub use import::{import_path_at, named_import_module_at, named_imported_names_at, uri_to_path};

#[derive(Clone, Debug)]
pub struct RelatedLocation {
    pub message: String,
    pub uri: String,
    pub line: u32,
    pub col: u32,
}

#[derive(Clone, Debug)]
pub struct LspDiag {
    pub message: String,
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub severity: u8,
    pub code: Option<varn_core::ErrorCode>,
    pub related: Vec<RelatedLocation>,
    pub suggestions: Vec<varn_core::Suggestion>,
}

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
    uri: &'a str,
    ty: &'a Type,
}

/// `Type::Dynamic` as a borrowable constant, for symbols the checker left
/// untyped.
fn dynamic_ty() -> &'static Type {
    use std::sync::OnceLock;
    // `Type` is `Rc`-based and therefore not `Sync`; the analysis thread is the
    // only thread that ever reads this, and `OnceLock` here would require it.
    // A thread-local leak gives a `'static` borrow without that bound.
    thread_local! {
        static DYNAMIC: &'static Type = Box::leak(Box::new(Type::Dynamic));
    }
    let _ = std::marker::PhantomData::<OnceLock<()>>;
    DYNAMIC.with(|d| *d)
}

impl std::fmt::Debug for SymbolView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SymbolView")
            .field("name", &self.name())
            .field("kind", &self.kind())
            .finish()
    }
}

impl<'a> SymbolView<'a> {
    pub fn name(&self) -> &'a str {
        self.sym.name.as_ref()
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
        self.sym.doc.as_deref()
    }
    pub fn origin(&self) -> Option<&'a str> {
        self.sym.origin_module.as_deref()
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
        self.sym.type_params.iter().map(|s| s.to_string()).collect()
    }
    pub fn is_arrow(&self) -> bool {
        matches!(&self.ty.0, varn_core::TypeKind::Fn(ft) if ft.is_arrow)
    }
    pub fn is_from_stdlib(&self) -> bool {
        self.origin().is_some_and(|m| {
            m.starts_with("std:") || m.starts_with("core:") || m.starts_with("runtime:")
        })
    }
    /// A function reads as its return type; everything else as its own.
    pub fn type_str(&self) -> String {
        match (&self.ty.0, self.kind()) {
            (varn_core::TypeKind::Fn(ft), SymbolKind::Function | SymbolKind::Method) => {
                ft.return_type.to_string()
            }
            _ => self.ty.to_string(),
        }
    }
    pub fn params_str(&self) -> String {
        match &self.ty.0 {
            varn_core::TypeKind::Fn(ft) => ft
                .params
                .iter()
                .map(|p| {
                    format!(
                        "{}: {}{}",
                        p.name.as_deref().unwrap_or("_"),
                        p.ty,
                        if p.optional { "?" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join(", "),
            _ => String::new(),
        }
    }
    pub fn global_key(&self, is_global: bool) -> String {
        crate::pipeline::stable_global_key(
            self.uri,
            self.name(),
            self.kind(),
            Some(self.id),
            self.origin(),
            self.sym.original_name.as_deref(),
            is_global,
        )
    }
}

#[derive(Clone, Debug)]
pub struct TokenRecord {
    pub kind: TokenKind,
    pub line: u32,
    pub col: u32,
    pub length: u32,
    pub offset: u32,
    pub lexeme: String,
}

#[derive(Debug)]
pub enum ChainResult<'a> {
    Symbol(SymbolView<'a>),
    /// A member, as the checker described it.
    ///
    /// One variant, not two. There used to be a borrowed `Member` (a pointer
    /// into the mirrored member table) beside an owned `DynamicMember` (built
    /// when the mirror had no entry); with the mirror gone there is nothing to
    /// borrow and nothing to distinguish.
    Member {
        member: varn_checker::ResolvedMemberSummary,
        parent_name: String,
    },
}

impl ChainResult<'_> {
    pub fn name(&self) -> &str {
        match self {
            ChainResult::Symbol(s) => &s.name(),
            ChainResult::Member { member, .. } => &member.name,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImportPathContext {
    pub prefix: String,
    pub specifier: String,
    pub content_start_col: u32,
}

pub struct SemanticDB {
    pub expr_types: FxHashMap<u32, varn_checker::ExprInfo>,

    pub node_scopes: FxHashMap<u32, varn_checker::ScopeId>,

    pub symbol_types: FxHashMap<varn_checker::SymbolId, varn_checker::Type>,

    pub arena: SymbolArena,

    pub scopes: ScopeArena,

    pub global_scope: varn_checker::ScopeId,

    pub flattened_members: FxHashMap<String, Vec<varn_checker::types::ClassMemberInfo>>,

    pub member_resolutions: FxHashMap<u32, varn_checker::MemberResolution>,

    pub call_resolutions: FxHashMap<u32, varn_checker::CallResolution>,

    pub bind: varn_checker::BindResult,
}

impl SemanticDB {
    pub fn resolve_at(
        &self,
        name: &str,
        cursor_offset: u32,
    ) -> Option<(varn_checker::SymbolId, varn_checker::Type)> {
        let scope_id = self.scope_at_offset(cursor_offset);
        let scope = self.scopes.get(scope_id);
        let sym_id = scope.resolve(name, &self.scopes)?;
        let ty = self
            .symbol_types
            .get(&sym_id)
            .cloned()
            .or_else(|| self.arena.get(sym_id).ty.clone())
            .unwrap_or(varn_checker::Type::Dynamic);
        Some((sym_id, ty))
    }

    pub fn scope_at_offset(&self, cursor_offset: u32) -> varn_checker::ScopeId {
        let mut best_scope = self.global_scope;
        let mut best_offset: u32 = 0;
        for (&offset, &scope) in &self.node_scopes {
            if offset <= cursor_offset && offset >= best_offset {
                best_offset = offset;
                best_scope = scope;
            }
        }
        best_scope
    }
}

pub struct DocumentState {
    pub source: String,
    pub uri: String,
    pub diagnostics: Vec<LspDiag>,
    /// The symbols this document declares or imports, by arena id.
    ///
    /// Ids, not records: the symbol itself lives in `db.arena`, and what the
    /// editor adds to it is derived on demand by [`SymbolView`].
    pub symbols: Vec<varn_checker::SymbolId>,
    /// The type each symbol resolved to during this analysis.
    ///
    /// Kept as one map rather than cloned into every symbol. It preserves the
    /// pipeline's original rule exactly: the type recorded for the symbol's own
    /// offset when it is not `dynamic`, else the declared type.
    pub resolved_types: FxHashMap<varn_checker::SymbolId, Type>,
    pub tokens: Vec<TokenRecord>,
    /// Comments, in source order. Parallel to `tokens`, never mixed into them —
    /// see [`varn_core::Trivia`].
    pub trivia: Vec<varn_core::Trivia>,
    pub symbol_map: HashMap<String, SymbolKind>,

    pub type_param_names: HashSet<String>,

    pub db: SemanticDB,

    pub import_paths: Vec<String>,
    pub positional_index: crate::queries::indexes::PositionalIndex,
    pub ast: Option<varn_core::ast::Program>,
}

// `DocumentState` is deliberately neither `Send` nor `Sync`. It is built on
// `Rc` throughout — `BindResult`, `Type`, every interned name — so sharing one
// across threads races on non-atomic refcounts. It used to carry
// `unsafe impl Send`/`Sync`, which did not make that safe; it silenced the
// check that forbade it.
//
// Its owner is the analysis thread (`crate::analysis`), and the missing impls
// are what keep it there: a request handler that tried to return one from the
// analysis closure fails to compile.

pub type DocumentAnalysis = DocumentState;

impl DocumentState {
    /// The members reachable on `sym`, asked of the checker.
    ///
    /// Replaces `SymbolView<'_>::members`, a member table this crate used to
    /// build eagerly for every symbol on every keystroke, with its own chain of
    /// cross-module fallbacks — a second, tooling-only answer to a question
    /// `get_members_of_type` already answers, and answers better (generics
    /// substituted, extensions included, signatures from the declaration).
    ///
    /// A type-shaped symbol (class, interface, enum, namespace, struct) is
    /// asked about *by name*: `sym.ty()` for a class is the type of the class
    /// itself, not of its instances.
    pub fn members_of(
        &self,
        sym: SymbolView<'_>,
    ) -> Vec<varn_checker::ResolvedMemberSummary> {
        let ty = match sym.kind() {
            SymbolKind::Class
            | SymbolKind::Interface
            | SymbolKind::Enum
            | SymbolKind::Struct
            | SymbolKind::Namespace => varn_checker::Type::named(sym.name().to_owned()),
            _ => sym.ty().clone(),
        };
        self.members_of_type(&ty)
    }

    /// The members reachable on `ty`, asked of the checker.
    pub fn members_of_type(&self, ty: &Type) -> Vec<varn_checker::ResolvedMemberSummary> {
        crate::workspace::resolver::with_resolver(|r| {
            varn_checker::get_members_of_type(r, ty, &self.db.bind)
        })
    }

    /// Every symbol of this document, as a view over the checker's arena.
    pub fn symbols(&self) -> impl Iterator<Item = SymbolView<'_>> + '_ {
        let ids = &self.symbols;
        (0..ids.len()).map(move |i| self.symbol(ids[i]))
    }

    /// One symbol, as a view.
    pub fn symbol(&self, id: varn_checker::SymbolId) -> SymbolView<'_> {
        SymbolView {
            id,
            sym: self.db.arena.get(id),
            uri: &self.uri,
            ty: self
                .resolved_types
                .get(&id)
                .or(self.db.arena.get(id).ty.as_ref())
                .unwrap_or_else(|| dynamic_ty()),
        }
    }

    pub fn resolve_symbol_id_at_offset(&self, offset: u32) -> Option<varn_checker::SymbolId> {
        if let Some(info) = self.db.expr_types.get(&offset) {
            if let Some(sid) = info.symbol_id {
                return Some(sid);
            }
        }
        let token = self.tokens.iter().find(|t| t.offset == offset)?;
        if let Some((sid, _)) = self.db.resolve_at(&token.lexeme, token.offset) {
            return Some(sid);
        }
        if let Some(sid) = self
            .db
            .arena
            .find_id_by_name_and_line(&token.lexeme, token.line + 1)
        {
            return Some(sid);
        }

        None
    }

    pub fn symbol_global_key_for_id(&self, id: varn_checker::SymbolId) -> Option<String> {
        if id >= self.db.arena.len() {
            return None;
        }
        let sym = self.db.arena.get(id);
        let name = sym.name.as_ref();
        let kind = sym.kind;
        let origin = sym.origin_module.as_deref();
        let original_name = sym.original_name.as_deref();

        if let Some(origin_mod) = origin {
            let canonical_name = original_name.unwrap_or(name);
            let origin_uri = if origin_mod.starts_with("file://")
                || origin_mod.starts_with("std:")
                || origin_mod.starts_with("core:")
                || origin_mod.starts_with("runtime:")
            {
                origin_mod.to_owned()
            } else {
                varn_modules::resolver::path_to_uri(origin_mod)
            };
            return Some(format!("m:{}#{kind:?}:{}", origin_uri, canonical_name));
        }

        let is_global = self
            .db
            .scopes
            .get(self.db.global_scope)
            .bindings
            .values()
            .any(|&sid| sid == id);
        let norm_uri = if self.uri.starts_with("file://")
            || self.uri.starts_with("std:")
            || self.uri.starts_with("core:")
            || self.uri.starts_with("runtime:")
        {
            self.uri.to_owned()
        } else {
            varn_modules::resolver::path_to_uri(&self.uri)
        };

        if is_global {
            Some(format!("m:{}#{kind:?}:{}", norm_uri, name))
        } else {
            Some(format!("u:{}#{kind:?}:{}", norm_uri, id))
        }
    }

    pub fn token_global_key(&self, offset: u32) -> Option<String> {
        if let Some(token) = self.tokens.iter().find(|t| t.offset == offset) {
            if let Some((parent_name, member)) = self.member_at_pos(token.line, token.col) {
                return Some(format!("member:{}:{}", parent_name, member.name));
            }
        }
        let sid = self.resolve_symbol_id_at_offset(offset)?;
        self.symbol_global_key_for_id(sid)
    }
}
