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
        self.scan_number_token(tokens, lexemes);
        let following = self.src.get(self.pos).copied();
        if let Some(problem) = number_problem(&self.src[start..self.pos], following) {
            let end = if following.is_some_and(is_suffix_byte) {
                let mut end = self.pos;
                while self.src.get(end).copied().is_some_and(is_suffix_byte) {
                    end += 1;
                }
                end
            } else {
                self.pos
            };
            self.report_span(
                vn_core::ErrorCode::InvalidNumericLiteral,
                problem,
                start,
                end,
            );
        }
    }

    fn scan_number_token(&mut self, tokens: &mut Vec<TokenRecord>, lexemes: &mut Vec<u8>) {
        let start = self.pos;
        let (sl, sc) = self.location();

        if self.peek(0) == b'0' && (self.peek(1) == b'b' || self.peek(1) == b'B') {
            self.cur_col += 2;
            self.pos += 2;
            while is_binary_digit(self.peek(0)) || self.peek(0) == b'_' {
                self.cur_col += 1;
                self.pos += 1;
            }
            let bigint = self.peek(0) == b'n';
            if bigint {
                self.cur_col += 1;
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
            self.cur_col += 2;
            self.pos += 2;
            while is_octal_digit(self.peek(0)) || self.peek(0) == b'_' {
                self.cur_col += 1;
                self.pos += 1;
            }
            let bigint = self.peek(0) == b'n';
            if bigint {
                self.cur_col += 1;
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
            self.cur_col += 2;
            self.pos += 2;
            while is_hex_digit(self.peek(0)) || self.peek(0) == b'_' {
                self.cur_col += 1;
                self.pos += 1;
            }
            let bigint = self.peek(0) == b'n';
            if bigint {
                self.cur_col += 1;
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
            self.cur_col += 1;
            self.pos += 1;
        }
        let mut is_float = false;

        if self.peek(0) == b'.' && is_digit(self.peek(1)) {
            is_float = true;
            self.cur_col += 1;
            self.pos += 1;
            while is_digit(self.peek(0)) || self.peek(0) == b'_' {
                self.cur_col += 1;
                self.pos += 1;
            }
        }

        if self.peek(0) == b'e' || self.peek(0) == b'E' {
            is_float = true;
            self.cur_col += 1;
            self.pos += 1;
            if self.peek(0) == b'+' || self.peek(0) == b'-' {
                self.cur_col += 1;
                self.pos += 1;
            }
            while is_digit(self.peek(0)) {
                self.cur_col += 1;
                self.pos += 1;
            }
        }
        let bigint = self.peek(0) == b'n';
        let decimal_suffix = !bigint && self.peek(0) == b'd';
        if bigint || decimal_suffix {
            self.cur_col += 1;
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

        if quote == b'"' && self.peek(1) == b'"' && self.peek(2) == b'"' {
            self.cur_col += 3;
            self.pos += 3;
            let mut content_start = self.pos;
            if !self.is_eof() && self.peek(0) == b'\r' && self.peek(1) == b'\n' {
                self.cur_line += 1;
                self.cur_col = 1;
                self.pos += 2;
                content_start = self.pos;
            } else if !self.is_eof() && self.peek(0) == b'\n' {
                self.cur_line += 1;
                self.cur_col = 1;
                self.pos += 1;
                content_start = self.pos;
            }

            while !self.is_eof() {
                if self.peek(0) == b'"' && self.peek(1) == b'"' && self.peek(2) == b'"' {
                    break;
                }
                if self.peek(0) == b'\n' {
                    self.cur_line += 1;
                    self.cur_col = 1;
                    self.pos += 1;
                } else if self.peek(0) == b'\r' && self.peek(1) == b'\n' {
                    self.cur_line += 1;
                    self.cur_col = 1;
                    self.pos += 2;
                } else {
                    self.advance_byte();
                }
            }
            let content_end = self.pos;
            if self.is_eof() {
                self.report_unterminated(
                    vn_core::ErrorCode::UnterminatedString,
                    "unterminated string literal",
                    start,
                );
            } else {
                self.cur_col += 3;
                self.pos += 3;
            }
            let (el, ec) = self.location();
            push_token(
                tokens,
                lexemes,
                self.src,
                crate::token_kind::RAW_STR,
                sl,
                sc,
                start as u32,
                el,
                ec,
                self.pos as u32,
                content_start,
                content_end,
            );
            return;
        }

        self.cur_col += 1;
        self.pos += 1;
        let content_start = self.pos;

        while !self.is_eof() && self.peek(0) != quote {
            if self.peek(0) == b'\\' {
                self.cur_col += 1;
                self.pos += 1;
                if !self.is_eof() {
                    self.advance_byte();
                }
            } else {
                self.advance_byte();
            }
        }
        let content_end = self.pos;
        if self.is_eof() {
            self.report_unterminated(
                vn_core::ErrorCode::UnterminatedString,
                "unterminated string literal",
                start,
            );
        } else {
            self.cur_col += 1;
            self.pos += 1;
        }

        let (el, ec) = self.location();

        let kind = if quote == b'\'' && is_single_char(&self.src[content_start..content_end]) {
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

        if self.src[self.pos].is_ascii() {
            self.cur_col += 1;
            self.pos += 1;
        } else if let Ok(s) = std::str::from_utf8(&self.src[self.pos..]) {
            if let Some(ch) = s.chars().next() {
                let len = ch.len_utf8();
                self.cur_col += len as u32;
                self.pos += len;
            } else {
                self.cur_col += 1;
                self.pos += 1;
            }
        } else {
            self.cur_col += 1;
            self.pos += 1;
        }

        while !self.is_eof() {
            let c = self.peek(0);
            if c.is_ascii() {
                if is_identifier_part(c) {
                    self.cur_col += 1;
                    self.pos += 1;
                } else {
                    break;
                }
            } else if let Ok(s) = std::str::from_utf8(&self.src[self.pos..]) {
                if let Some(ch) = s.chars().next() {
                    if ch.is_alphanumeric() || ch == '_' {
                        let len = ch.len_utf8();
                        self.cur_col += len as u32;
                        self.pos += len;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
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

fn is_single_char(content: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(content) else {
        return false;
    };
    let mut chars = text.chars();
    match chars.next() {
        Some('\\') => {
            let escape = chars.as_str();
            escape.chars().count() == 1
                || (escape.starts_with("u{")
                    && escape.ends_with('}')
                    && escape.matches('}').count() == 1)
        }
        Some(_) => chars.next().is_none(),
        None => false,
    }
}

fn is_suffix_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn number_problem(text: &[u8], following: Option<u8>) -> Option<&'static str> {
    if following.is_some_and(is_suffix_byte) {
        return Some("a number can only end with `n` (bigint) or `d` (decimal), not with other letters or digits");
    }
    let (digits, prefixed) = match text {
        [b'0', b'x' | b'X' | b'b' | b'B' | b'o' | b'O', rest @ ..] => (rest, true),
        _ => (text, false),
    };
    let digits = match digits {
        [body @ .., b'n' | b'd'] => body,
        _ => digits,
    };
    if prefixed && digits.iter().all(|byte| *byte == b'_') {
        return Some("a prefixed number needs at least one digit after the prefix");
    }
    if digits.windows(2).any(|pair| pair == b"__")
        || digits.first() == Some(&b'_')
        || digits.last() == Some(&b'_')
    {
        return Some("the digit separator `_` must sit between two digits");
    }
    if !prefixed
        && digits.windows(2).any(|pair| {
            (pair[0] == b'_' && !pair[1].is_ascii_digit())
                || (pair[1] == b'_' && !pair[0].is_ascii_digit())
        })
    {
        return Some("the digit separator `_` must sit between two digits");
    }
    if !prefixed && matches!(digits.last(), Some(b'e' | b'E' | b'+' | b'-')) {
        return Some("an exponent needs digits after `e`");
    }
    None
}
