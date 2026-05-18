use varn_checker::SymbolKind;

use crate::document::{DocumentState, MemberRecord, MethodHoverInfo};

pub fn member_at(
    state: &DocumentState,
    line: u32,
    col: u32,
) -> Option<(String, SymbolKind, &MemberRecord)> {
    state.member_at_pos(line, col)
}

pub fn members_of<'a>(state: &'a DocumentState, class_name: &str) -> Option<&'a [MemberRecord]> {
    state
        .symbols
        .iter()
        .find(|s| s.name == class_name)
        .map(|s| s.members.as_slice())
}

pub fn method_info_at(state: &DocumentState, line: u32, col: u32) -> Option<MethodHoverInfo> {
    state.method_at_pos(line, col)
}
