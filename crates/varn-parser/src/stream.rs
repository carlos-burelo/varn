use crate::ParseProfile;
use std::rc::Rc;
use varn_core::ast::{AstId, AstTypeKind, Expr, ExprKind, Stmt, StmtKind, TypeNode};
use varn_core::{ErrorCode, ParsedNumber, SourceRange, Token, TokenKind};

pub struct TokenStream {
    tokens: Vec<Token>,
    lexeme_buf: Rc<[u8]>,
    pos: usize,
    pub filename: Rc<str>,

    pending_doc: Option<Rc<str>>,

    pub errors: varn_core::DiagnosticBag,
    pub profile: ParseProfile,

    split_count: u8,
    next_ast_id: AstId,

    /// Owns the dedup table for every name the parser mints: identifiers,
    /// raw literal text, labels, property keys. Lives here rather than on
    /// `Parser` because the whole recursive-descent parser is a tree of free
    /// functions threaded with `&mut TokenStream`, not `&mut Parser` —
    /// exactly why `errors` and `pending_doc` live here too. `Parser`
    /// derefs to `TokenStream`, so `parser.interner` still resolves.
    pub interner: varn_core::AtomInterner,
}

impl TokenStream {
    /// `interner` is owned by the caller: a fresh `AtomInterner::new()` for a
    /// self-contained parse (benchmarks, macros, isolated tests), or a clone
    /// of a compilation-wide table when this parse's `Atom`s must compare
    /// equal to another module's (see `varn_core::AtomInterner` and
    /// `DiskResolver::interner_snapshot`). Either way `TokenStream` never
    /// mints its own — doing that per file is exactly the bug this replaced.
    pub fn new(
        tokens: Vec<Token>,
        lexeme_buf: Rc<[u8]>,
        filename: Rc<str>,
        interner: varn_core::AtomInterner,
    ) -> Self {
        TokenStream {
            tokens,
            lexeme_buf,
            pos: 0,
            filename,
            pending_doc: None,
            errors: varn_core::DiagnosticBag::new(),
            profile: ParseProfile::default(),
            split_count: 0,
            next_ast_id: 1,
            interner,
        }
    }

    #[inline(always)]
    pub fn next_ast_id(&mut self) -> AstId {
        let id = self.next_ast_id;
        self.next_ast_id += 1;
        id
    }

    #[inline(always)]
    pub fn expr(&mut self, range: SourceRange, kind: ExprKind) -> Expr {
        Expr::new(self.next_ast_id(), range, kind)
    }

    #[inline(always)]
    pub fn stmt(&mut self, range: SourceRange, kind: StmtKind) -> Stmt {
        Stmt::new(self.next_ast_id(), range, kind)
    }

    #[inline(always)]
    pub fn type_node(&mut self, range: SourceRange, kind: AstTypeKind) -> TypeNode {
        TypeNode {
            id: self.next_ast_id(),
            range,
            kind,
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

    /// Cursor into the token vector. Used by error recovery to detect that a
    /// pass made no progress, which is what keeps the program loop from
    /// spinning on a token it just failed to parse.
    #[inline]
    pub fn pos(&self) -> usize {
        self.pos
    }

    #[inline]
    pub fn line(&self) -> u32 {
        self.token().range.start.line
    }

    #[inline]
    pub fn prev_line(&self) -> u32 {
        if self.pos > 0 {
            self.tokens[self.pos - 1].range.end.line
        } else {
            0
        }
    }

    /// Zero-width range at the end of the previous token.
    ///
    /// Where a diagnostic belongs when the offending thing is the *absence* of
    /// a token: anchoring it to the current token would underline whatever
    /// followed — often the next, perfectly valid, line.
    #[inline]
    pub fn prev_end_range(&self) -> SourceRange {
        if self.pos == 0 {
            return self.range();
        }
        let end = self.tokens[self.pos - 1].range.end;
        SourceRange { start: end, end }
    }

    #[inline]
    pub fn column(&self) -> u32 {
        self.token().range.start.column
    }

    #[inline]
    pub fn prev_range(&self) -> SourceRange {
        if self.pos > 0 {
            self.tokens[self.pos - 1].range
        } else {
            self.range()
        }
    }

    #[inline]
    pub fn peek_line(&self, offset: usize) -> u32 {
        let idx = self.pos.saturating_add(offset);
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
    pub fn consume(&mut self) -> Token {
        let tok = self.token().clone();
        self.advance();
        tok
    }

    #[inline]
    pub fn parsed_num(&self) -> Option<ParsedNumber> {
        self.token().parsed_num
    }

    #[inline]
    pub fn advance(&mut self) {
        if self.split_count > 0 {
            self.split_count -= 1;
            return;
        }
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
    }

    #[inline]
    pub fn check(&self, kind: TokenKind) -> bool {
        self.kind() == kind
    }

    #[inline]
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

    pub fn expect_token(&mut self, kind: TokenKind) -> Result<Token, String> {
        if self.check(kind) {
            let tok = self.token().clone();
            self.advance();
            Ok(tok)
        } else {
            let tok = self.token();
            let lex = tok.get_lexeme(&self.lexeme_buf);
            Err(format!(
                "Expected {:?}, got {:?} ({:?}) at {}:{}",
                kind, tok.kind, lex, tok.range.start.line, tok.range.start.column
            ))
        }
    }

    pub fn expect_id(&mut self) -> Result<varn_core::Atom, String> {
        if self.kind().can_be_identifier() {
            let id = self.consume_lexeme();
            Ok(id)
        } else {
            let tok = self.token();
            let lex = tok.get_lexeme(&self.lexeme_buf);
            Err(format!(
                "Expected identifier, got {:?} ({:?}) at {}:{}",
                tok.kind, lex, tok.range.start.line, tok.range.start.column
            ))
        }
    }

    /// Interns the current token's lexeme and advances past it. The
    /// canonical source of `Atom`s during parsing — every identifier, raw
    /// literal, label and property key that ends up in the AST is minted
    /// here, so any two occurrences of the same text share one `Atom`.
    pub fn consume_lexeme(&mut self) -> varn_core::Atom {
        let tok = self.token().clone();
        let text = tok.get_lexeme(&self.lexeme_buf);
        let atom = self.interner.intern(text);
        self.advance();
        atom
    }

    #[inline]
    pub fn is_eof(&self) -> bool {
        self.kind() == TokenKind::EOF
    }

    #[inline]
    pub fn span_from(&self, start: SourceRange) -> SourceRange {
        if self.pos == 0 {
            return start;
        }
        let prev = &self.tokens[self.pos - 1];
        SourceRange {
            start: start.start,
            end: prev.range.end,
        }
    }

    pub fn eat_semicolon(&mut self) {
        while self.eat(TokenKind::Semicolon) {}
    }

    pub fn expect_semicolon(&mut self) -> Result<(), String> {
        if self.eat(TokenKind::Semicolon) {
            while self.eat(TokenKind::Semicolon) {}
            return Ok(());
        }
        if self.is_eof() || self.check(TokenKind::RBrace) {
            return Ok(());
        }
        let tok = self.token();
        let prev = if self.pos > 0 {
            Some(&self.tokens[self.pos - 1])
        } else {
            None
        };
        if let Some(prev) = prev {
            if prev.range.end.line < tok.range.start.line {
                return Ok(());
            }
        }
        let lex = tok.get_lexeme(&self.lexeme_buf);
        Err(format!(
            "Expected ';', got {:?} ({:?}) at {}:{}",
            tok.kind, lex, tok.range.start.line, tok.range.start.column
        ))
    }

    pub fn store_pending_doc(&mut self, doc: Rc<str>) {
        self.pending_doc = Some(doc);
    }

    pub fn take_pending_doc(&mut self) -> Option<Rc<str>> {
        self.pending_doc.take()
    }

    pub fn current_doc(&mut self) -> Option<String> {
        self.pending_doc.take().map(|rc| rc.to_string())
    }

    #[inline]
    pub fn check_rangle(&self) -> bool {
        self.split_count > 0
            || matches!(
                self.token().kind,
                TokenKind::RAngle | TokenKind::GtGt | TokenKind::GtGtGt
            )
    }

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

    #[inline]
    pub fn save(&self) -> (usize, u8) {
        (self.pos, self.split_count)
    }

    #[inline]
    pub fn restore(&mut self, state: (usize, u8)) {
        self.pos = state.0;
        self.split_count = state.1;
    }
}
