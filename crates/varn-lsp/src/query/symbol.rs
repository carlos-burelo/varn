use crate::document::{DocumentState, SymbolRecord};

use super::member::member_at;
use super::token::token_at;

pub fn symbol_at_line(state: &DocumentState, line: u32) -> Option<&SymbolRecord> {
    state.symbols.iter().find(|s| s.line == line)
}

pub fn symbols_named<'a>(state: &'a DocumentState, name: &str) -> Vec<&'a SymbolRecord> {
    state.symbols.iter().filter(|s| s.name == name).collect()
}

pub fn symbol_at(state: &DocumentState, line: u32, col: u32) -> Option<&SymbolRecord> {
    let tok = token_at(state, line, col)?;

    if member_at(state, line, col).is_some() {
        return None;
    }

    if let Some(info) = state.db.expr_types.get(&tok.offset) {
        if let Some(sid) = info.symbol_id {
            return state.symbols.iter().find(|s| s.symbol_id == Some(sid));
        }
    }

    if let Some((sid, _)) = state.db.resolve_at(&tok.lexeme, tok.offset) {
        return state.symbols.iter().find(|s| s.symbol_id == Some(sid));
    }

    None
}
