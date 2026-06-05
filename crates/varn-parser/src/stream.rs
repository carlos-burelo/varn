use crate::ParseProfile;
use std::rc::Rc;
use varn_core::{ErrorCode, ParsedNumber, SourceRange, Token, TokenKind};

pub struct TokenStream {
    tokens: Vec<Token>,
    lexeme_buf: Rc<[u8]>,
    pos: usize,
    pub filename: Rc<str>,

    pending_doc: Option<Rc<str>>,

    pub errors: varn_core::DiagnosticBag,
    pub profile: ParseProfile,

    // Tracks how many synthetic RAngle tokens remain after splitting a >> or >>> token.
    // Allows `Array<Task<T>>` to parse correctly without the lexer needing context.
    split_count: u8,
}

impl TokenStream {
    pub fn new(tokens: Vec<Token>, lexeme_buf: Rc<[u8]>, filename: Rc<str>) -> Self {
        TokenStream {
            tokens,
            lexeme_buf,
            pos: 0,
            filename,
            pending_doc: None,
            errors: varn_core::DiagnosticBag::new(),
            profile: ParseProfile::default(),
            split_count: 0,
        }
    }

    pub fn push_error(&mut self, message: String, range: SourceRange) {
        self.errors
            .error(ErrorCode::UnexpectedToken, message, range);
    }

    #[inline]
    pub fn kind(&self) -> TokenKind {
        if self.split_count > 0 {
            return TokenKind::RAngle;
        }
        self.token().kind
    }

    #[inline]
    pub fn peek_kind(&self, offset: usize) -> TokenKind {
        let idx = self.pos + offset;
        if idx < self.tokens.len() {
            self.tokens[idx].kind
        } else {
            TokenKind::EOF
        }
    }

    #[inline]
    pub fn peek_line(&self, offset: usize) -> u32 {
        let idx = self.pos + offset;
        if idx < self.tokens.len() {
            self.tokens[idx].range.start.line
        } else {
            if let Some(last) = self.tokens.last() {
                last.range.end.line
            } else {
                0
            }
        }
    }

    #[inline]
    pub fn token(&self) -> &Token {
        if self.pos < self.tokens.len() {
            &self.tokens[self.pos]
        } else {
            self.tokens.last().expect("empty token stream")
        }
    }

    #[inline]
    pub fn range(&self) -> SourceRange {
        self.token().range
    }

    #[inline]
    pub fn range_from(&self, start: SourceRange) -> SourceRange {
        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].range
        } else {
            start
        };
        start.to(end)
    }

    #[inline]
    pub fn lexeme(&self) -> &str {
        let tok = self.token();
        tok.get_lexeme(&self.lexeme_buf)
    }

    pub fn consume_str(&mut self) -> String {
        let s = self.lexeme().to_owned();
        self.advance();
        s
    }

    #[inline]
    pub fn parsed_num(&self) -> Option<ParsedNumber> {
        self.token().parsed_num
    }

    #[inline]
    pub fn check(&self, kind: TokenKind) -> bool {
        self.kind() == kind
    }

    #[inline]
    pub fn is_eof(&self) -> bool {
        self.kind() == TokenKind::EOF
    }

    pub fn advance(&mut self) {
        if self.split_count > 0 {
            self.split_count -= 1;
            return;
        }
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
    }

    pub fn consume(&mut self) -> Token {
        if self.pos < self.tokens.len() {
            let t = self.tokens[self.pos].clone();
            self.pos += 1;
            t
        } else {
            self.tokens.last().cloned().expect("empty token stream")
        }
    }

    pub fn consume_lexeme(&mut self) -> std::rc::Rc<str> {
        let lexeme = varn_core::intern_string(self.lexeme());
        self.advance();
        lexeme
    }

    pub fn eat(&mut self, kind: TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    pub fn expect(&mut self, kind: TokenKind) -> Result<(), String> {
        if self.check(kind) {
            self.advance();
            Ok(())
        } else {
            let tok = self.token();
            let lex = tok.get_lexeme(&self.lexeme_buf);
            Err(format!(
                "Expected {:?}, got {:?} ({:?}) at {}:{}",
                kind, tok.kind, lex, tok.range.start.line, tok.range.start.column
            ))
        }
    }

    pub fn expect_lexeme(&mut self, kind: TokenKind) -> Result<std::rc::Rc<str>, String> {
        if self.check(kind) {
            Ok(self.consume_lexeme())
        } else {
            let tok = self.token();
            let lex = tok.get_lexeme(&self.lexeme_buf);
            Err(format!(
                "Expected {:?}, got {:?} ({:?}) at {}:{}",
                kind, tok.kind, lex, tok.range.start.line, tok.range.start.column
            ))
        }
    }

    pub fn expect_token(&mut self, kind: TokenKind) -> Result<Token, String> {
        if self.check(kind) {
            Ok(self.consume())
        } else {
            let tok = self.token();
            let lex = tok.get_lexeme(&self.lexeme_buf);
            Err(format!(
                "Expected {:?}, got {:?} ({:?}) at {}:{}",
                kind, tok.kind, lex, tok.range.start.line, tok.range.start.column
            ))
        }
    }

    pub fn expect_id(&mut self) -> Result<std::rc::Rc<str>, String> {
        if self.kind().can_be_identifier() {
            Ok(self.consume_lexeme())
        } else {
            let tok = self.token();
            let lex = tok.get_lexeme(&self.lexeme_buf);
            Err(format!(
                "Expected Identifier, got {:?} ({:?}) at {}:{}",
                tok.kind, lex, tok.range.start.line, tok.range.start.column
            ))
        }
    }

    pub fn peek_expect(&self, kind: TokenKind) -> Result<(), String> {
        if self.check(kind) {
            Ok(())
        } else {
            let tok = self.token();
            Err(format!(
                "Expected {:?}, got {:?} at {}:{}",
                kind, tok.kind, tok.range.start.line, tok.range.start.column
            ))
        }
    }

    pub fn line(&self) -> u32 {
        self.token().range.start.line
    }

    pub fn column(&self) -> u32 {
        self.token().range.start.column
    }

    pub fn eat_semicolon(&mut self) {
        self.eat(TokenKind::Semicolon);
    }

    pub fn prev_line(&self) -> u32 {
        if self.pos > 0 {
            self.tokens[self.pos - 1].range.start.line
        } else {
            0
        }
    }

    pub fn is_arrow_ahead(&self) -> bool {
        self.check(TokenKind::FatArrow)
    }

    pub fn save(&self) -> usize {
        self.pos
    }

    pub fn restore(&mut self, p: usize) {
        self.pos = p;
        self.split_count = 0;
    }

    #[inline]
    pub fn lexeme_buf(&self) -> &[u8] {
        &self.lexeme_buf
    }

    pub fn store_pending_doc(&mut self, doc: Rc<str>) {
        self.pending_doc = Some(doc);
    }

    pub fn take_pending_doc(&mut self) -> Option<Rc<str>> {
        self.pending_doc.take()
    }

    /// True when current token closes a generic type arg list.
    /// Handles split `>>` and `>>>` so nested generics like `Array<Task<T>>` parse correctly.
    #[inline]
    pub fn check_rangle(&self) -> bool {
        if self.split_count > 0 {
            return true;
        }
        matches!(
            self.token().kind,
            TokenKind::RAngle | TokenKind::GtGt | TokenKind::GtGtGt
        )
    }

    /// Consume one closing `>`, splitting `>>` into two virtual tokens.
    pub fn eat_rangle(&mut self) -> bool {
        if self.split_count > 0 {
            self.split_count -= 1;
            return true;
        }
        match self.token().kind {
            TokenKind::RAngle => {
                self.pos += 1;
                true
            }
            TokenKind::GtGt => {
                self.split_count = 1;
                self.pos += 1;
                true
            }
            TokenKind::GtGtGt => {
                self.split_count = 2;
                self.pos += 1;
                true
            }
            _ => false,
        }
    }

    pub fn expect_rangle(&mut self) -> Result<(), String> {
        if self.eat_rangle() {
            Ok(())
        } else {
            let tok = self.token();
            let lex = tok.get_lexeme(&self.lexeme_buf);
            Err(format!(
                "Expected RAngle, got {:?} ({:?}) at {}:{}",
                tok.kind, lex, tok.range.start.line, tok.range.start.column
            ))
        }
    }
}
