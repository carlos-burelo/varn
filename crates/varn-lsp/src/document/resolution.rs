use varn_checker::symbol::SymbolId;
use varn_core::TokenKind;

use super::{DocumentState, SymbolRecord, TokenRecord};

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
            if let Some(sid) = info.symbol_id.filter(|sid| *sid < self.db.arena.len()) {
                let arena_sym = self.db.arena.get(sid);
                if arena_sym.name.as_ref() == tok.lexeme.as_str() {
                    return Some(sid);
                }
            }
        }

        self.db
            .resolve_at(&tok.lexeme, tok.offset)
            .map(|(sid, _)| sid)
    }

    pub fn checker_symbol_id_at(&self, line: u32, col: u32) -> Option<SymbolId> {
        let tok = self.identifier_token_at(line, col)?;
        self.checker_symbol_id_at_token(tok)
    }

    pub fn checker_symbol_at(&self, line: u32, col: u32) -> Option<&SymbolRecord> {
        let sid = self.checker_symbol_id_at(line, col)?;
        self.symbols.iter().find(|s| s.symbol_id == Some(sid))
    }
}
