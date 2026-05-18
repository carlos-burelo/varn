use crate::token_kind::{REGEX, TEMPLATE, TEMPLATE_HEAD, TEMPLATE_MIDDLE, TEMPLATE_TAIL};
use crate::token_record::{push_token, TokenRecord};

impl super::Scanner<'_> {
    pub(super) fn scan_template_literal(
        &mut self,
        tokens: &mut Vec<TokenRecord>,
        lexemes: &mut Vec<u8>,
    ) {
        let start = self.pos;
        let (sl, sc) = self.location();
        self.pos += 1;

        let kind = self.scan_template_chunk(false);
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

        if kind == TEMPLATE_HEAD {
            self.template_depth += 1;
            self.brace_depth.push(0);
        }
    }

    pub(super) fn scan_template_continuation(
        &mut self,
        rbrace_pos: usize,
        tokens: &mut Vec<TokenRecord>,
        lexemes: &mut Vec<u8>,
    ) {
        let (sl, sc) = self.location_at(rbrace_pos);

        let kind = self.scan_template_chunk(true);
        let (el, ec) = self.location();

        push_token(
            tokens,
            lexemes,
            self.src,
            kind,
            sl,
            sc,
            rbrace_pos as u32,
            el,
            ec,
            self.pos as u32,
            rbrace_pos,
            self.pos,
        );

        if kind == TEMPLATE_MIDDLE {
            self.template_depth += 1;
            self.brace_depth.push(0);
        }
    }

    pub(super) fn scan_template_chunk(&mut self, continuation: bool) -> u32 {
        loop {
            if self.is_eof() {
                return if continuation {
                    TEMPLATE_TAIL
                } else {
                    TEMPLATE
                };
            }
            match self.peek(0) {
                b'`' => {
                    self.pos += 1;
                    return if continuation {
                        TEMPLATE_TAIL
                    } else {
                        TEMPLATE
                    };
                }
                b'$' if self.peek(1) == b'{' => {
                    self.pos += 2;
                    return if continuation {
                        TEMPLATE_MIDDLE
                    } else {
                        TEMPLATE_HEAD
                    };
                }
                b'\\' => {
                    self.pos += 1;
                    if !self.is_eof() {
                        self.pos += 1;
                    }
                }
                _ => {
                    self.pos += 1;
                }
            }
        }
    }

    pub(super) fn scan_regex(&mut self, tokens: &mut Vec<TokenRecord>, lexemes: &mut Vec<u8>) {
        let start = self.pos;
        let (sl, sc) = self.location();
        self.pos += 1;

        let mut in_char_class = false;
        let mut escaped = false;

        loop {
            if self.is_eof() {
                break;
            }
            let c = self.peek(0);
            if escaped {
                self.pos += 1;
                escaped = false;
                continue;
            }
            match c {
                b'\\' => {
                    escaped = true;
                    self.pos += 1;
                }
                b'[' if !in_char_class => {
                    in_char_class = true;
                    self.pos += 1;
                }
                b']' if in_char_class => {
                    in_char_class = false;
                    self.pos += 1;
                }
                b'/' if !in_char_class => {
                    self.pos += 1;
                    break;
                }
                b'\n' => break,
                _ => {
                    self.pos += 1;
                }
            }
        }

        while !self.is_eof() {
            let c = self.peek(0);
            if c.is_ascii_lowercase() {
                self.pos += 1;
            } else {
                break;
            }
        }

        let (el, ec) = self.location();
        push_token(
            tokens,
            lexemes,
            self.src,
            REGEX,
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
