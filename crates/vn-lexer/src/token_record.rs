pub(crate) struct TokenRecord {
    pub(crate) kind: u32,
    pub(crate) start_line: u32,
    pub(crate) start_col: u32,
    pub(crate) start_offset: u32,
    pub(crate) end_line: u32,
    pub(crate) end_col: u32,
    pub(crate) end_offset: u32,
    pub(crate) lex_offset: u32,
    pub(crate) lex_len: u32,
}

#[inline]
pub(crate) fn push_token(
    tokens: &mut Vec<TokenRecord>,
    lexemes: &mut Vec<u8>,
    src: &[u8],
    kind: u32,
    sl: u32,
    sc: u32,
    so: u32,
    el: u32,
    ec: u32,
    eo: u32,
    lex_start: usize,
    lex_end: usize,
) {
    let lex_offset = lexemes.len() as u32;
    let lex_len = (lex_end - lex_start) as u32;
    if lex_end > lex_start {
        lexemes.extend_from_slice(&src[lex_start..lex_end]);
    }
    tokens.push(TokenRecord {
        kind,
        start_line: sl,
        start_col: sc,
        start_offset: so,
        end_line: el,
        end_col: ec,
        end_offset: eo,
        lex_offset,
        lex_len,
    });
}
