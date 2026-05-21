use crate::ParseProfile;
use std::rc::Rc;
use varn_core::{ErrorCode, ParsedNumber, SourceRange, Token, TokenKind};

pub struct TokenStream {
    tokens: Vec<Token>,
    pos: usize,
    pub filename: Rc<str>,

    pending_doc: Option<Rc<str>>,

    pub errors: varn_core::DiagnosticBag,
    pub profile: ParseProfile,
}

impl TokenStream {
    pub fn new(tokens: Vec<Token>, filename: Rc<str>) -> Self {
        TokenStream {
            tokens,
            pos: 0,
            filename,
            pending_doc: None,
            errors: varn_core::DiagnosticBag::new(),
            profile: ParseProfile::default(),
        }
    }

    pub fn push_error(&mut self, message: String, range: SourceRange) {
        self.errors
            .error(ErrorCode::UnexpectedToken, message, range);
    }

    #[inline]
    pub fn kind(&self) -> TokenKind {
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

    /// Span from `start` to the end of the previously consumed token.
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
        &self.token().lexeme
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
        let lexeme = varn_core::intern_string(&self.token().lexeme);
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
            Err(format!(
                "Expected {:?}, got {:?} ({:?}) at {}:{}",
                kind, tok.kind, tok.lexeme, tok.range.start.line, tok.range.start.column
            ))
        }
    }

    pub fn expect_lexeme(&mut self, kind: TokenKind) -> Result<std::rc::Rc<str>, String> {
        if self.check(kind) {
            Ok(self.consume_lexeme())
        } else {
            let tok = self.token();
            Err(format!(
                "Expected {:?}, got {:?} ({:?}) at {}:{}",
                kind, tok.kind, tok.lexeme, tok.range.start.line, tok.range.start.column
            ))
        }
    }

    pub fn expect_token(&mut self, kind: TokenKind) -> Result<Token, String> {
        if self.check(kind) {
            Ok(self.consume())
        } else {
            let tok = self.token();
            Err(format!(
                "Expected {:?}, got {:?} ({:?}) at {}:{}",
                kind, tok.kind, tok.lexeme, tok.range.start.line, tok.range.start.column
            ))
        }
    }

    pub fn expect_id(&mut self) -> Result<std::rc::Rc<str>, String> {
        if self.kind().can_be_identifier() {
            Ok(self.consume_lexeme())
        } else {
            let tok = self.token();
            Err(format!(
                "Expected Identifier, got {:?} ({:?}) at {}:{}",
                tok.kind, tok.lexeme, tok.range.start.line, tok.range.start.column
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
    }

    pub fn store_pending_doc(&mut self, doc: Rc<str>) {
        self.pending_doc = Some(doc);
    }

    pub fn take_pending_doc(&mut self) -> Option<Rc<str>> {
        self.pending_doc.take()
    }
}
