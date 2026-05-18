use crate::token_kind::DOC_COMMENT;
use crate::token_record::{push_token, TokenRecord};

impl super::Scanner<'_> {
    pub(super) fn skip_line_comment(&mut self) {
        while !self.is_eof() && self.peek(0) != b'\n' {
            self.pos += 1;
        }
    }

    pub(super) fn skip_block_comment(&mut self) {
        while !self.is_eof() {
            if self.peek(0) == b'*' && self.peek(1) == b'/' {
                self.pos += 2;
                break;
            }
            self.pos += 1;
        }
    }

    pub(super) fn scan_doc_comment(
        &mut self,
        tokens: &mut Vec<TokenRecord>,
        lexemes: &mut Vec<u8>,
    ) {
        let tok_start = self.pos - 2;
        let (sl, sc) = self.location_at(tok_start);
        let so = tok_start as u32;

        self.pos += 1;

        let content_start = self.pos;
        while !self.is_eof() {
            if self.peek(0) == b'*' && self.peek(1) == b'/' {
                break;
            }
            self.pos += 1;
        }
        let content_end = self.pos;
        if !self.is_eof() {
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
