use crate::document::DocumentState;

/// The member at `line`/`col`, and the name of the type it belongs to.
pub fn member_at(
    state: &DocumentState,
    line: u32,
    col: u32,
) -> Option<(String, varn_checker::ResolvedMemberSummary)> {
    state.member_at_pos(line, col)
}
