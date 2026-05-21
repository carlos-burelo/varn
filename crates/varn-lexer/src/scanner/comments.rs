use crate::token_kind::DOC_COMMENT;
use crate::token_record::{push_token, TokenRecord};

impl super::Scanner<'_> {
    pub(super) fn skip_line_comment(&mut self) {
        // Line comment content never contains newlines (we stop at \n).
        while !self.is_eof() && self.peek(0) != b'\n' {
            self.cur_col += 1;
            self.pos += 1;
        }
    }

    pub(super) fn skip_block_comment(&mut self) {
        // Block comment CAN contain newlines.
        while !self.is_eof() {
            if self.peek(0) == b'*' && self.peek(1) == b'/' {
                // '*' and '/' — never newlines
                self.cur_col += 2;
                self.pos += 2;
                break;
            }
            self.advance_byte();
        }
    }

    pub(super) fn scan_doc_comment(
        &mut self,
        tokens: &mut Vec<TokenRecord>,
        lexemes: &mut Vec<u8>,
    ) {
        // At this point, '/*' has already been consumed by advance_bytes(2) in core.rs.
        // tok_start points to the '/' of '/**'.
        let tok_start = self.pos - 2;
        let (sl, sc) = self.location_at(tok_start);
        let so = tok_start as u32;

        // Skip the extra '*' that makes it a doc comment (already peeked in core.rs).
        // This '*' is never a newline.
        self.cur_col += 1;
        self.pos += 1;

        let content_start = self.pos;
        // Doc comment body CAN contain newlines.
        while !self.is_eof() {
            if self.peek(0) == b'*' && self.peek(1) == b'/' {
                break;
            }
            self.advance_byte();
        }
        let content_end = self.pos;
        if !self.is_eof() {
            // '*' and '/' — never newlines
            self.cur_col += 2;
            self.pos += 2;
        }

        let (el, ec) = self.location();
        let eo = self.pos as u32;

        push_token(
            tokens,
            lexemes,
            self.src,
            DOC_COMMENT,
            sl,
            sc,
            so,
            el,
            ec,
            eo,
            content_start,
            content_end,
        );
    }
}
