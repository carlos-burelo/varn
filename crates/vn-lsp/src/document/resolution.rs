use vn_core::TokenKind;
use vn_sem::symbol::SymbolId;

use super::{DocumentState, SymbolView, TokenRecord};

impl DocumentState {
    pub fn identifier_token_at(&self, line: u32, col: u32) -> Option<&TokenRecord> {
        self.tokens.iter().find(|t| {
            t.line == line
                && (t.kind == TokenKind::Identifier || t.kind.can_be_identifier())
                && t.col <= col
                && col < t.col + t.length
        })
    }

    pub fn checker_symbol_id_at_token(&self, tok: &TokenRecord) -> Option<SymbolId> {
        if let Some(info) = self.db.expr_types.get(&tok.offset)
            && let Some(sid) = info.symbol_id.filter(|sid| *sid < self.db.bind.arena.len())
        {
            let arena_sym = self.db.bind.arena.get(sid);
            if self.name(arena_sym.name) == self.lexeme(tok) {
                return Some(sid);
            }
        }

        self.db
            .resolve_at(self.lexeme(tok), tok.offset)
            .map(|(sid, _)| sid)
    }

    pub fn checker_symbol_id_at(&self, line: u32, col: u32) -> Option<SymbolId> {
        let tok = self.identifier_token_at(line, col)?;
        self.checker_symbol_id_at_token(tok)
    }

    pub fn checker_symbol_at(&self, line: u32, col: u32) -> Option<SymbolView<'_>> {
        let sid = self.checker_symbol_id_at(line, col)?;
        self.symbols().find(|s| s.id == sid)
    }
}

impl DocumentState {
    pub fn source_text(&self, range: vn_core::SourceRange) -> &str {
        let (start, end) = (range.start.offset as usize, range.end.offset as usize);
        self.source.get(start..end).unwrap_or("")
    }
}

impl DocumentState {
    pub fn type_node_decl_name(&self, node: &vn_core::ast::TypeNode) -> Option<&str> {
        match &node.kind {
            vn_core::TypeKind::Named(name, _) | vn_core::TypeKind::Generic(name, _, _) => {
                Some(self.name(*name))
            }
            vn_core::TypeKind::Primitive(_)
            | vn_core::TypeKind::Literal(_)
            | vn_core::TypeKind::This
            | vn_core::TypeKind::Array(_)
            | vn_core::TypeKind::Union(_)
            | vn_core::TypeKind::Intersection(_)
            | vn_core::TypeKind::Tuple(_)
            | vn_core::TypeKind::TemplateLiteral(_)
            | vn_core::TypeKind::Fn(_)
            | vn_core::TypeKind::Record(_)
            | vn_core::TypeKind::Interface(_)
            | vn_core::TypeKind::Typeof(_)
            | vn_core::TypeKind::KeyOf(_)
            | vn_core::TypeKind::IndexedAccess { .. }
            | vn_core::TypeKind::Mapped { .. }
            | vn_core::TypeKind::Conditional { .. }
            | vn_core::TypeKind::Infer(_)
            | vn_core::TypeKind::EnumVariant { .. }
            | vn_core::TypeKind::TypePredicate { .. } => None,
        }
    }
}
