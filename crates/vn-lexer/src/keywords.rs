use crate::token_kind::IDENTIFIER;

pub(crate) fn keyword_kind(raw: &[u8]) -> u32 {
    std::str::from_utf8(raw)
        .ok()
        .and_then(vn_core::TokenKind::keyword)
        .map_or(IDENTIFIER, |kind| kind as u32)
}
