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
        let tok = self.identifier_token_at(line, col)?;
        if let Some(sid) = self.checker_symbol_id_at(line, col) {
            if let Some(s) = self.symbols.iter().find(|s| s.symbol_id == Some(sid)) {
                if s.kind == varn_checker::SymbolKind::Function || s.kind == varn_checker::SymbolKind::Method {
                    if let Some(cls) = self.symbols.iter().find(|c| {
                        c.name == tok.lexeme
                            && matches!(
                                c.kind,
                                varn_checker::SymbolKind::Class
                                    | varn_checker::SymbolKind::Struct
                                    | varn_checker::SymbolKind::Interface
                            )
                    }) {
                        return Some(cls);
                    }
                }
                return Some(s);
            }
        }

        self.symbols
            .iter()
            .find(|s| {
                s.name == tok.lexeme
                    && matches!(
                        s.kind,
                        varn_checker::SymbolKind::Class
                            | varn_checker::SymbolKind::Struct
                            | varn_checker::SymbolKind::Interface
                            | varn_checker::SymbolKind::Enum
                    )
            })
            .or_else(|| self.symbols.iter().find(|s| s.name == tok.lexeme))
    }
}
