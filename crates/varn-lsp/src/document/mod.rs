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
    pub related: Vec<RelatedLocation>,
    pub suggestions: Vec<varn_core::Suggestion>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberKind {
    Constructor,
    Method,
    Function,
    Property,
    Variable,
    EnumMember,
    Getter,
    Setter,
    Class,
    Interface,
    Namespace,
    Enum,
    Struct,
}

#[derive(Clone, Debug)]
pub struct MemberRecord {
    pub name: String,
    pub type_str: String,
    pub params_str: String,
    pub is_static: bool,
    pub is_optional: bool,
    pub kind: MemberKind,
    pub is_arrow: bool,
    pub is_async: bool,
    pub is_generator: bool,
    pub line: u32,
    pub col: u32,
    pub init_value: String,
    pub ty: Type,
    pub symbol_id: Option<varn_checker::symbol::SymbolId>,
    pub members: Vec<MemberRecord>,
}

#[derive(Clone, Debug)]
pub struct SymbolRecord {
    pub name: String,
    pub kind: SymbolKind,
    pub type_str: String,
    pub params_str: String,
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub has_explicit_type: bool,
    pub is_async: bool,
    pub is_generator: bool,
    pub is_arrow: bool,
    pub doc: Option<String>,
    pub members: Vec<MemberRecord>,
    pub type_params: Vec<String>,
    pub ty: Type,
    pub symbol_id: Option<varn_checker::symbol::SymbolId>,
    pub global_key: String,
    pub full_range: varn_core::SourceRange,
    pub is_from_stdlib: bool,
    pub origin: Option<String>,
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
    Symbol(&'a SymbolRecord),
    Member {
        member: &'a MemberRecord,
        parent_name: String,
    },
    DynamicMember {
        member: MemberRecord,
        parent_name: String,
    },
}

impl<'a> ChainResult<'a> {
    pub fn name(&self) -> &str {
        match self {
            ChainResult::Symbol(s) => &s.name,
            ChainResult::Member { member, .. } => &member.name,
            ChainResult::DynamicMember { member, .. } => &member.name,
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

    pub extension_members: HashMap<String, Vec<MemberRecord>>,
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
    pub symbols: Vec<SymbolRecord>,
    pub tokens: Vec<TokenRecord>,
    pub symbol_map: HashMap<String, SymbolKind>,

    pub type_param_names: HashSet<String>,

    pub db: SemanticDB,

    pub import_paths: Vec<String>,
    pub positional_index: crate::queries::indexes::PositionalIndex,
}

unsafe impl Send for DocumentState {}
unsafe impl Sync for DocumentState {}

pub type DocumentAnalysis = DocumentState;

impl DocumentState {
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
            if let Some((parent_name, _, member)) = self.member_at_pos(token.line, token.col) {
                return Some(format!("member:{}:{}", parent_name, member.name));
            }
        }
        let sid = self.resolve_symbol_id_at_offset(offset)?;
        self.symbol_global_key_for_id(sid)
    }
}
