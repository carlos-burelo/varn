use varn_checker::SymbolKind;

use crate::document::{DocumentState, MemberRecord};

pub fn member_at(
    state: &DocumentState,
    line: u32,
    col: u32,
) -> Option<(String, SymbolKind, &MemberRecord)> {
    state.member_at_pos(line, col)
}
