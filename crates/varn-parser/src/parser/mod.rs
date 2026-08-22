pub mod decls;
mod patterns;
mod stmt_decls;
mod stmts;

pub use decls::parse_class_decl;
pub use patterns::{parse_params, parse_single_param};
pub use stmts::parse_block;

use crate::stream::TokenStream;
use crate::ParseProfile;
use std::rc::Rc;
#[cfg(feature = "profiling")]
use std::time::Instant;
use varn_core::ast::Program;
use varn_core::{ErrorCode, TokenKind};

pub struct Parser {
    pub stream: TokenStream,
    pub diagnostics: varn_core::DiagnosticBag,
}

impl Parser {
    pub fn new(tokens: Vec<varn_core::Token>, lexeme_buf: Rc<[u8]>, filename: Rc<str>) -> Self {
        Parser {
            stream: TokenStream::new(tokens, lexeme_buf, filename),
            diagnostics: varn_core::DiagnosticBag::new(),
        }
    }

    pub fn parse_program(&mut self) -> Result<Program, varn_core::DiagnosticBag> {
        let (prog, errs, _) = self.parse_program_partial_with_profile();
        if errs.is_empty() {
            Ok(prog)
        } else {
            Err(errs)
        }
    }

    pub fn parse_program_with_profile(
        &mut self,
    ) -> Result<(Program, ParseProfile), varn_core::DiagnosticBag> {
        let (prog, errs, profile) = self.parse_program_partial_with_profile();
        if errs.is_empty() {
            Ok((prog, profile))
        } else {
            Err(errs)
        }
    }

    pub fn parse_program_partial(&mut self) -> (Program, varn_core::DiagnosticBag) {
        let (prog, errs, _) = self.parse_program_partial_with_profile();
        (prog, errs)
    }

    pub fn parse_program_partial_with_profile(
        &mut self,
    ) -> (Program, varn_core::DiagnosticBag, ParseProfile) {
        let range = self.stream.range();
        let mut body = vec![];
        #[cfg(feature = "profiling")]
        let started = Instant::now();

        while !self.stream.is_eof() {
            match self.parse_stmt_or_decl() {
                Ok(stmt) => body.push(stmt),
                Err(msg) => {
                    self.diagnostics
                        .error(ErrorCode::InvalidStatement, msg, self.stream.range());
                    self.recover();
                }
            }
        }
        #[cfg(feature = "profiling")]
        {
            self.stream.profile.program_loop += started.elapsed();
        }

        self.diagnostics
            .extend(std::mem::take(&mut self.stream.errors));
        let errors = self.diagnostics.clone();
        let profile = self.stream.profile.clone();

        let prog = Program {
            filename: self.stream.filename.clone(),
            body,
            range,
            metadata: Default::default(),
        };
        (prog, errors, profile)
    }

    fn recover(&mut self) {
        #[cfg(feature = "profiling")]
        let started = Instant::now();
        let mut depth: i32 = 0;
        loop {
            match self.stream.kind() {
                TokenKind::EOF => break,
                TokenKind::Semicolon if depth == 0 => {
                    self.stream.advance();
                    break;
                }
                TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket => {
                    depth += 1;
                    self.stream.advance();
                }
                TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket => {
                    if depth == 0 {
                        self.stream.advance();
                        break;
                    }
                    depth -= 1;
                    self.stream.advance();
                }
                TokenKind::Function
                | TokenKind::Class
                | TokenKind::Let
                | TokenKind::Const
                | TokenKind::Var
                | TokenKind::Import
                | TokenKind::Export
                | TokenKind::Return
                | TokenKind::If
                | TokenKind::For
                | TokenKind::While
                    if depth == 0 =>
                {
                    break
                }
                _ => {
                    self.stream.advance();
                }
            }
        }
        #[cfg(feature = "profiling")]
        {
            self.stream.profile.recover += started.elapsed();
        }
    }

    fn parse_stmt_or_decl(&mut self) -> Result<varn_core::ast::Stmt, String> {
        #[cfg(feature = "profiling")]
        let started = Instant::now();
        let s = &mut self.stream;
        while s.eat(TokenKind::Semicolon) {}
        if s.is_eof() {
            return Ok(s.stmt(
                s.range(),
                varn_core::ast::StmtKind::Empty,
            ));
        }
        let parsed = stmts::parse_stmt_or_decl_inner(s);
        #[cfg(feature = "profiling")]
        {
            self.stream.profile.stmt_or_decl += started.elapsed();
        }
        parsed
    }
}
