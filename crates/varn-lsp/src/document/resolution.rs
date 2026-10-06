use varn_checker::symbol::SymbolId;
use varn_core::TokenKind;

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
        if let Some(info) = self.db.expr_types.get(&tok.offset) {
            if let Some(sid) = info.symbol_id.filter(|sid| *sid < self.db.bind.arena.len()) {
                let arena_sym = self.db.bind.arena.get(sid);
                if self.name(arena_sym.name) == self.lexeme(tok) {
                    return Some(sid);
                }
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
    pub fn source_text(&self, range: varn_core::SourceRange) -> &str {
        let (start, end) = (range.start.offset as usize, range.end.offset as usize);
        self.source.get(start..end).unwrap_or("")
    }
}

impl DocumentState {
    pub fn type_node_decl_name(&self, node: &varn_core::ast::TypeNode) -> Option<&str> {
        match &node.kind {
            varn_core::TypeKind::Named(name, _) | varn_core::TypeKind::Generic(name, _, _) => {
                Some(self.name(*name))
            }
            _ => None,
        }
    }
}
