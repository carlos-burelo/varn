use crate::char_class::{
    is_binary_digit, is_digit, is_hex_digit, is_identifier_part, is_octal_digit,
};
use crate::keywords::keyword_kind;
use crate::token_kind::{
    BIGINT_LITERAL, BINARY_LITERAL, CHAR_TOK, DECIMAL_LITERAL, FLOAT_LITERAL, HEX_LITERAL,
    INTEGER_LITERAL, OCTAL_LITERAL, STR,
};
use crate::token_record::{push_token, TokenRecord};

impl super::Scanner<'_> {
    pub(super) fn scan_number(&mut self, tokens: &mut Vec<TokenRecord>, lexemes: &mut Vec<u8>) {
        let start = self.pos;
        let (sl, sc) = self.location_at(start);

        if self.peek(0) == b'0' && (self.peek(1) == b'b' || self.peek(1) == b'B') {
            self.pos += 2;
            while is_binary_digit(self.peek(0)) || self.peek(0) == b'_' {
                self.pos += 1;
            }
            let bigint = self.peek(0) == b'n';
            if bigint {
                self.pos += 1;
            }
            let (el, ec) = self.location();
            push_token(
                tokens,
                lexemes,
                self.src,
                if bigint {
                    BIGINT_LITERAL
                } else {
                    BINARY_LITERAL
                },
                sl,
                sc,
                start as u32,
                el,
                ec,
                self.pos as u32,
                start,
                self.pos,
            );
            return;
        }

        if self.peek(0) == b'0' && (self.peek(1) == b'o' || self.peek(1) == b'O') {
            self.pos += 2;
            while is_octal_digit(self.peek(0)) || self.peek(0) == b'_' {
                self.pos += 1;
            }
            let bigint = self.peek(0) == b'n';
            if bigint {
                self.pos += 1;
            }
            let (el, ec) = self.location();
            push_token(
                tokens,
                lexemes,
                self.src,
                if bigint {
                    BIGINT_LITERAL
                } else {
                    OCTAL_LITERAL
                },
                sl,
                sc,
                start as u32,
                el,
                ec,
                self.pos as u32,
                start,
                self.pos,
            );
            return;
        }

        if self.peek(0) == b'0' && (self.peek(1) == b'x' || self.peek(1) == b'X') {
            self.pos += 2;
            while is_hex_digit(self.peek(0)) || self.peek(0) == b'_' {
                self.pos += 1;
            }
            let bigint = self.peek(0) == b'n';
            if bigint {
                self.pos += 1;
            }
            let (el, ec) = self.location();
            push_token(
                tokens,
                lexemes,
                self.src,
                if bigint { BIGINT_LITERAL } else { HEX_LITERAL },
                sl,
                sc,
                start as u32,
                el,
                ec,
                self.pos as u32,
                start,
                self.pos,
            );
            return;
        }

        while is_digit(self.peek(0)) || self.peek(0) == b'_' {
            self.pos += 1;
        }
        let mut is_float = false;

        if self.peek(0) == b'.' && is_digit(self.peek(1)) {
            is_float = true;
            self.pos += 1;
            while is_digit(self.peek(0)) || self.peek(0) == b'_' {
                self.pos += 1;
            }
        }

        if self.peek(0) == b'e' || self.peek(0) == b'E' {
            is_float = true;
            self.pos += 1;
            if self.peek(0) == b'+' || self.peek(0) == b'-' {
                self.pos += 1;
            }
            while is_digit(self.peek(0)) {
                self.pos += 1;
            }
        }
        let bigint = self.peek(0) == b'n';
        let decimal_suffix = !bigint && self.peek(0) == b'd';
        if bigint || decimal_suffix {
            self.pos += 1;
        }
        let (el, ec) = self.location();
        let kind = if bigint {
            BIGINT_LITERAL
        } else if decimal_suffix {
            DECIMAL_LITERAL
        } else if is_float {
            FLOAT_LITERAL
        } else {
            INTEGER_LITERAL
        };
        push_token(
            tokens,
            lexemes,
            self.src,
            kind,
            sl,
            sc,
            start as u32,
            el,
            ec,
            self.pos as u32,
            start,
            self.pos,
        );
    }

    pub(super) fn scan_string(
        &mut self,
        quote: u8,
        tokens: &mut Vec<TokenRecord>,
        lexemes: &mut Vec<u8>,
    ) {
        let start = self.pos;
        let (sl, sc) = self.location();
        self.pos += 1;
        let content_start = self.pos;

        while !self.is_eof() && self.peek(0) != quote {
            if self.peek(0) == b'\\' {
                self.pos += 1;
                if !self.is_eof() {
                    self.pos += 1;
                }
            } else {
                self.pos += 1;
            }
        }
        let content_end = self.pos;
        if !self.is_eof() {
            self.pos += 1;
        }

        let (el, ec) = self.location();

        let kind = if quote == b'\'' && (content_end - content_start) == 1 {
            CHAR_TOK
        } else {
            STR
        };

        push_token(
            tokens,
            lexemes,
            self.src,
            kind,
            sl,
            sc,
            start as u32,
            el,
            ec,
            self.pos as u32,
            content_start,
            content_end,
        );
    }

    pub(super) fn scan_identifier(&mut self, tokens: &mut Vec<TokenRecord>, lexemes: &mut Vec<u8>) {
        let start = self.pos;
        let (sl, sc) = self.location();

        self.pos += 1;
        while !self.is_eof() && is_identifier_part(self.peek(0)) {
            self.pos += 1;
        }

        let raw = &self.src[start..self.pos];
        let kind = keyword_kind(raw);
        let (el, ec) = self.location();
        push_token(
            tokens,
            lexemes,
            self.src,
            kind,
            sl,
            sc,
            start as u32,
            el,
            ec,
            self.pos as u32,
            start,
            self.pos,
        );
    }
}
