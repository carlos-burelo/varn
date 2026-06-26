use crate::document::{DocumentState, SymbolRecord};

use super::member::member_at;
use super::token::token_at;

pub fn symbol_at(state: &DocumentState, line: u32, col: u32) -> Option<&SymbolRecord> {
    let tok = token_at(state, line, col)?;

    if member_at(state, line, col).is_some() {
        return None;
    }

    state
        .checker_symbol_at(line, col)
        .filter(|sym| sym.name == tok.lexeme)
}
