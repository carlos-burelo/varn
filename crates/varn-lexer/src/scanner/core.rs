use crate::char_class::{is_digit, is_identifier_start};
use crate::token_kind::{
    BIGINT_LITERAL, BINARY_LITERAL, CHAR_TOK, DECIMAL_LITERAL, DOC_COMMENT, EOF, FALSE,
    FLOAT_LITERAL, HEX_LITERAL, IDENTIFIER, INTEGER_LITERAL, LBRACE, MINUSMINUS, NULL,
    OCTAL_LITERAL, PLUSPLUS, RBRACE, RBRACKET, REGEX, RPAREN, STR, SUPER, TEMPLATE, TEMPLATE_TAIL,
    THIS, TRUE,
};
use crate::token_record::{push_token, TokenRecord};

impl super::Scanner<'_> {
    pub(super) fn can_start_regex(&self) -> bool {
        if self.last_kind == u32::MAX {
            return true;
        }
        let k = self.last_kind;

        if k == IDENTIFIER
            || k == INTEGER_LITERAL
            || k == FLOAT_LITERAL
            || k == BINARY_LITERAL
            || k == OCTAL_LITERAL
            || k == HEX_LITERAL
            || k == BIGINT_LITERAL
            || k == DECIMAL_LITERAL
            || k == STR
            || k == CHAR_TOK
            || k == TEMPLATE
            || k == TEMPLATE_TAIL
            || k == REGEX
            || k == RPAREN
            || k == RBRACKET
            || k == RBRACE
            || k == TRUE
            || k == FALSE
            || k == NULL
            || k == THIS
            || k == SUPER
            || k == PLUSPLUS
            || k == MINUSMINUS
        {
            return false;
        }
        true
    }

    pub(crate) fn scan_all(&mut self) -> (Vec<TokenRecord>, Vec<u8>) {
        let mut tokens: Vec<TokenRecord> = Vec::with_capacity(2048);
        let mut lexemes: Vec<u8> = Vec::with_capacity(8192);

        loop {
            while !self.is_eof() {
                let c = self.peek(0);
                if c == b' ' || c == b'\t' || c == b'\r' || c == b'\n' {
                    self.pos += 1;
                } else {
                    break;
                }
            }

            if self.is_eof() {
                break;
            }

            let c = self.peek(0);

            if c == b'/' {
                let next = self.peek(1);
                if next == b'/' {
                    self.pos += 2;
                    self.skip_line_comment();
                    continue;
                }
                if next == b'*' {
                    self.pos += 2;

                    if self.peek(0) == b'*' && self.peek(1) != b'/' {
                        self.scan_doc_comment(&mut tokens, &mut lexemes);
                        self.last_kind = DOC_COMMENT;
                    } else {
                        self.skip_block_comment();
                    }
                    continue;
                }

                if self.can_start_regex() && next != b'=' {
                    self.scan_regex(&mut tokens, &mut lexemes);
                } else {
                    self.scan_operator(&mut tokens, &mut lexemes);
                }
                self.last_kind = tokens.last().map(|t| t.kind).unwrap_or(self.last_kind);
                continue;
            }

            if is_digit(c) {
                self.scan_number(&mut tokens, &mut lexemes);
                self.last_kind = tokens.last().map(|t| t.kind).unwrap_or(self.last_kind);
                continue;
            }

            if c == b'"' || c == b'\'' {
                self.scan_string(c, &mut tokens, &mut lexemes);
                self.last_kind = tokens.last().map(|t| t.kind).unwrap_or(self.last_kind);
                continue;
            }

            if c == b'`' {
                self.scan_template_literal(&mut tokens, &mut lexemes);
                self.last_kind = tokens.last().map(|t| t.kind).unwrap_or(self.last_kind);
                continue;
            }

            if is_identifier_start(c) {
                self.scan_identifier(&mut tokens, &mut lexemes);
                self.last_kind = tokens.last().map(|t| t.kind).unwrap_or(self.last_kind);
                continue;
            }

            if c == b'{' && self.template_depth > 0 {
                let top = self.brace_depth.len() - 1;
                self.brace_depth[top] += 1;
                self.scan_operator(&mut tokens, &mut lexemes);
                self.last_kind = LBRACE;
                continue;
            }

            if c == b'}' && self.template_depth > 0 {
                let depth = *self.brace_depth.last().unwrap_or(&0);
                if depth == 0 {
                    self.brace_depth.pop();
                    self.template_depth -= 1;
                    let rbrace_pos = self.pos;
                    self.pos += 1;
                    self.scan_template_continuation(rbrace_pos, &mut tokens, &mut lexemes);
                    self.last_kind = tokens.last().map(|t| t.kind).unwrap_or(self.last_kind);
                    continue;
                } else if let Some(depth_ref) = self.brace_depth.last_mut() {
                    *depth_ref -= 1;
                    self.scan_operator(&mut tokens, &mut lexemes);
                    self.last_kind = RBRACE;
                    continue;
                } else {
                    let (line, col) = self.location();
                    self.diagnostics.error(
                        varn_core::ErrorCode::UnterminatedBrace,
                        "unexpected '}' outside template expression",
                        varn_core::SourceRange::at(line, col, self.pos as u32),
                    );
                }
            }

            self.scan_operator(&mut tokens, &mut lexemes);
            self.last_kind = tokens.last().map(|t| t.kind).unwrap_or(self.last_kind);
        }

        let (eof_l, eof_c) = self.location();
        let eof_off = self.pos as u32;
        push_token(
            &mut tokens,
            &mut lexemes,
            self.src,
            EOF,
            eof_l,
            eof_c,
            eof_off,
            eof_l,
            eof_c,
            eof_off,
            self.pos,
            self.pos,
        );

        (tokens, lexemes)
    }
}
