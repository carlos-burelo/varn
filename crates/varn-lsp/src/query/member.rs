use crate::document::DocumentState;


pub fn member_at(
    state: &DocumentState,
    line: u32,
    col: u32,
) -> Option<(String, varn_checker::ResolvedMemberSummary)> {
    state.member_at_pos(line, col)
}
