mod chain_queries;
pub mod classes;
pub mod import;
pub mod position;
mod resolution;
mod semantic_db;
mod symbol_queries;
mod symbol_view;
mod types;

use rustc_hash::{FxHashMap, FxHashSet};

use varn_checker::{SymbolKind, Type};
use varn_core::TokenKind;

pub use import::{import_path_at, named_import_module_at, named_imported_names_at, uri_to_path};
pub use semantic_db::SemanticDB;
pub use symbol_view::SymbolView;

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

#[derive(Clone, Debug)]
pub struct TokenRecord {
    pub kind: TokenKind,
    pub line: u32,
    pub col: u32,
    pub length: u32,
    pub offset: u32,

    pub end: u32,
}

pub fn token_lexeme<'a>(source: &'a str, tok: &TokenRecord) -> &'a str {
    &source[tok.offset as usize..tok.end as usize]
}

#[derive(Debug)]
pub enum ChainResult<'a> {
    Symbol(SymbolView<'a>),

    Member {
        member: varn_checker::ResolvedMemberSummary,
        parent_name: String,
    },
}

impl ChainResult<'_> {
    pub fn name(&self) -> &str {
        match self {
            ChainResult::Symbol(s) => s.name(),
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SymbolTarget {
    Local {
        uri: String,
        symbol_id: varn_checker::SymbolId,
    },
    Global {
        origin: String,
        canonical_name: String,
    },
    Member {
        parent_name: String,
        member_name: String,
    },
}

pub struct DocumentState {
    pub source: String,
    pub uri: String,
    pub diagnostics: Vec<LspDiag>,

    pub symbols: Vec<varn_checker::SymbolId>,
    pub tokens: Vec<TokenRecord>,

    pub trivia: Vec<varn_core::Trivia>,
    pub symbol_map: FxHashMap<String, SymbolKind>,

    pub type_param_names: FxHashSet<String>,

    pub db: SemanticDB,

    pub import_paths: Vec<String>,
    pub spatial_index: crate::query::SpatialIndex,
    pub ast: Option<varn_core::ast::Program>,

    pub ast_arena: varn_core::ast::AstArena,
}

pub type DocumentAnalysis = DocumentState;

impl DocumentState {
    pub fn members_of(&self, sym: SymbolView<'_>) -> Vec<varn_checker::ResolvedMemberSummary> {
        let ty = match sym.kind() {
            SymbolKind::Class
            | SymbolKind::Interface
            | SymbolKind::Enum
            | SymbolKind::Struct
            | SymbolKind::Namespace => self.db.named_type(sym.name()),
            _ => *sym.ty(),
        };
        self.members_of_type(&ty)
    }

    pub fn members_of_type(&self, ty: &Type) -> Vec<varn_checker::ResolvedMemberSummary> {
        crate::workspace::resolver::with_resolver(|r| {
            varn_checker::get_members_of_type(
                r,
                ty,
                &self.db.bind,
                std::sync::Arc::make_mut(&mut self.db.types.borrow_mut()),
            )
        })
    }

    pub fn name(&self, atom: varn_core::Atom) -> &str {
        self.db.name(atom)
    }

    pub fn lexeme(&self, tok: &TokenRecord) -> &str {
        token_lexeme(&self.source, tok)
    }

    pub fn ty_text(&self, ty: &Type) -> String {
        self.db.ty_text(ty)
    }

    pub fn symbols(&self) -> impl Iterator<Item = SymbolView<'_>> + '_ {
        let ids = &self.symbols;
        (0..ids.len()).map(move |i| self.symbol(ids[i]))
    }

    pub fn symbol(&self, id: varn_checker::SymbolId) -> SymbolView<'_> {
        SymbolView {
            id,
            sym: self.db.bind.arena.get(id),
            ty: self
                .db
                .symbol_types
                .get(&id)
                .or(self.db.bind.arena.get(id).ty.as_ref())
                .unwrap_or(&symbol_view::DYNAMIC_TY),
            db: &self.db,
        }
    }

    pub fn expr_entry_at_offset(&self, offset: u32) -> Option<&varn_checker::TypeEntry> {
        let ast_id = self.spatial_index.innermost_at(offset)?;
        self.db.expr_table.get(&ast_id)
    }

    pub fn expr_type_at_offset(&self, offset: u32) -> Option<&varn_checker::Type> {
        self.expr_entry_at_offset(offset).map(|e| &e.ty)
    }

    pub fn resolve_symbol_id_at_offset(&self, offset: u32) -> Option<varn_checker::SymbolId> {
        if let Some(entry) = self.expr_entry_at_offset(offset) {
            if let Some(sid) = entry.symbol_id {
                return Some(sid);
            }
        }
        if let Some(info) = self.db.expr_types.get(&offset) {
            if let Some(sid) = info.symbol_id {
                return Some(sid);
            }
        }
        let token = self.tokens.iter().find(|t| t.offset == offset)?;
        if let Some((sid, _)) = self.db.resolve_at(self.lexeme(token), token.offset) {
            return Some(sid);
        }
        let atom = self.db.bind.interner.get(self.lexeme(token));
        if let Some(sid) = atom.and_then(|a| {
            self.db
                .bind
                .arena
                .find_id_by_name_and_line(a, token.line + 1)
        }) {
            return Some(sid);
        }

        None
    }

    pub fn symbol_target_for_id(&self, id: varn_checker::SymbolId) -> Option<SymbolTarget> {
        if id >= self.db.bind.arena.len() {
            return None;
        }
        let sym = self.db.bind.arena.get(id);
        let name = self.name(sym.name).to_owned();
        if let Some(origin_mod) = sym.origin_module.map(|a| self.name(a)) {
            let canonical_name = sym
                .original_name
                .map(|a| self.name(a).to_owned())
                .unwrap_or_else(|| name.clone());
            let origin_uri = if origin_mod.starts_with("file://")
                || origin_mod.starts_with("std:")
                || origin_mod.starts_with("core:")
                || origin_mod.starts_with("runtime:")
            {
                origin_mod.to_string()
            } else {
                varn_modules::resolver::path_to_uri(origin_mod)
            };
            return Some(SymbolTarget::Global {
                origin: origin_uri,
                canonical_name,
            });
        }

        let is_global = self
            .db
            .bind
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
            self.uri.clone()
        } else {
            varn_modules::resolver::path_to_uri(&self.uri)
        };

        if is_global {
            Some(SymbolTarget::Global {
                origin: norm_uri,
                canonical_name: name,
            })
        } else {
            Some(SymbolTarget::Local {
                uri: norm_uri,
                symbol_id: id,
            })
        }
    }

    pub fn symbol_target_at_offset(&self, offset: u32) -> Option<SymbolTarget> {
        if let Some(token) = self.tokens.iter().find(|t| t.offset == offset) {
            if let Some((parent_name, member)) = self.member_at_pos(token.line, token.col) {
                return Some(SymbolTarget::Member {
                    parent_name,
                    member_name: member.name.to_string(),
                });
            }
        }
        let sid = self.resolve_symbol_id_at_offset(offset)?;
        self.symbol_target_for_id(sid)
    }
}
