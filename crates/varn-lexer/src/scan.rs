use crate::scanner::Scanner;

pub fn scan(
    source: &str,
    filename: &str,
) -> (
    Vec<varn_core::Token>,
    std::sync::Arc<[u8]>,
    Vec<varn_core::Diagnostic>,
) {
    let (tokens, buf, diags, _) =
        scan_inner(source, filename, crate::scanner::LexerConfig::default());
    (tokens, buf, diags)
}

pub fn scan_with_trivia(
    source: &str,
    filename: &str,
) -> (
    Vec<varn_core::Token>,
    std::sync::Arc<[u8]>,
    Vec<varn_core::Diagnostic>,
    Vec<varn_core::Trivia>,
) {
    scan_inner(
        source,
        filename,
        crate::scanner::LexerConfig {
            emit_trivia: true,
            ..Default::default()
        },
    )
}

fn parse_num_lexeme(kind: varn_core::TokenKind, lexeme: &str) -> Option<varn_core::ParsedNumber> {
    use varn_core::{ParsedNumber, TokenKind};
    let cleaned = lexeme.replace('_', "");
    match kind {
        TokenKind::IntegerLiteral => cleaned.parse::<i64>().ok().map(ParsedNumber::Int),
        TokenKind::BinaryLiteral => {
            let digits = cleaned
                .strip_prefix("0b")
                .or_else(|| cleaned.strip_prefix("0B"))?;
            i64::from_str_radix(digits, 2).ok().map(ParsedNumber::Int)
        }
        TokenKind::OctalLiteral => {
            let digits = cleaned
                .strip_prefix("0o")
                .or_else(|| cleaned.strip_prefix("0O"))?;
            i64::from_str_radix(digits, 8).ok().map(ParsedNumber::Int)
        }
        TokenKind::HexLiteral => {
            let digits = cleaned
                .strip_prefix("0x")
                .or_else(|| cleaned.strip_prefix("0X"))?;
            i64::from_str_radix(digits, 16).ok().map(ParsedNumber::Int)
        }
        TokenKind::FloatLiteral => cleaned.parse::<f64>().ok().map(ParsedNumber::Float),
        TokenKind::EOF
        | TokenKind::Dynamic
        | TokenKind::Identifier
        | TokenKind::BigIntLiteral
        | TokenKind::Str
        | TokenKind::Char
        | TokenKind::Template
        | TokenKind::TemplateHead
        | TokenKind::TemplateMiddle
        | TokenKind::TemplateTail
        | TokenKind::RegularExpression
        | TokenKind::LParen
        | TokenKind::RParen
        | TokenKind::LBrace
        | TokenKind::RBrace
        | TokenKind::LBracket
        | TokenKind::RBracket
        | TokenKind::LAngle
        | TokenKind::RAngle
        | TokenKind::Semicolon
        | TokenKind::Comma
        | TokenKind::Dot
        | TokenKind::DotDot
        | TokenKind::DotDotDot
        | TokenKind::DotDotEq
        | TokenKind::Colon
        | TokenKind::ColonColon
        | TokenKind::Question
        | TokenKind::QuestionDot
        | TokenKind::QuestionLBracket
        | TokenKind::QuestionQuestion
        | TokenKind::QuestionQuestionEq
        | TokenKind::Plus
        | TokenKind::PlusPlus
        | TokenKind::PlusEq
        | TokenKind::Minus
        | TokenKind::MinusMinus
        | TokenKind::MinusEq
        | TokenKind::Star
        | TokenKind::StarStar
        | TokenKind::StarEq
        | TokenKind::StarStarEq
        | TokenKind::Slash
        | TokenKind::SlashEq
        | TokenKind::Percent
        | TokenKind::PercentEq
        | TokenKind::Amp
        | TokenKind::AmpAmp
        | TokenKind::AmpEq
        | TokenKind::AmpAmpEq
        | TokenKind::Pipe
        | TokenKind::PipePipe
        | TokenKind::PipeEq
        | TokenKind::PipePipeEq
        | TokenKind::PipeGt
        | TokenKind::Caret
        | TokenKind::CaretEq
        | TokenKind::Tilde
        | TokenKind::LtLt
        | TokenKind::LtLtEq
        | TokenKind::GtGt
        | TokenKind::GtGtEq
        | TokenKind::GtGtGt
        | TokenKind::GtGtGtEq
        | TokenKind::Eq
        | TokenKind::EqEq
        | TokenKind::EqEqEq
        | TokenKind::Bang
        | TokenKind::BangEq
        | TokenKind::BangEqEq
        | TokenKind::Lt
        | TokenKind::LtEq
        | TokenKind::Gt
        | TokenKind::GtEq
        | TokenKind::Arrow
        | TokenKind::FatArrow
        | TokenKind::Let
        | TokenKind::Const
        | TokenKind::Var
        | TokenKind::Function
        | TokenKind::Class
        | TokenKind::Struct
        | TokenKind::Interface
        | TokenKind::Type
        | TokenKind::Enum
        | TokenKind::Namespace
        | TokenKind::Module
        | TokenKind::Extension
        | TokenKind::On
        | TokenKind::If
        | TokenKind::Else
        | TokenKind::Switch
        | TokenKind::Case
        | TokenKind::Default
        | TokenKind::While
        | TokenKind::For
        | TokenKind::Do
        | TokenKind::Break
        | TokenKind::Continue
        | TokenKind::Return
        | TokenKind::Throw
        | TokenKind::Try
        | TokenKind::Catch
        | TokenKind::Finally
        | TokenKind::Using
        | TokenKind::With
        | TokenKind::Import
        | TokenKind::Export
        | TokenKind::From
        | TokenKind::As
        | TokenKind::Async
        | TokenKind::Await
        | TokenKind::Yield
        | TokenKind::New
        | TokenKind::This
        | TokenKind::Super
        | TokenKind::Delete
        | TokenKind::Typeof
        | TokenKind::Instanceof
        | TokenKind::In
        | TokenKind::Of
        | TokenKind::Void
        | TokenKind::Is
        | TokenKind::True
        | TokenKind::False
        | TokenKind::Null
        | TokenKind::Public
        | TokenKind::Private
        | TokenKind::Protected
        | TokenKind::Static
        | TokenKind::Abstract
        | TokenKind::Override
        | TokenKind::Readonly
        | TokenKind::Declare
        | TokenKind::Native
        | TokenKind::Extends
        | TokenKind::Implements
        | TokenKind::Get
        | TokenKind::Set
        | TokenKind::Constructor
        | TokenKind::Destructor
        | TokenKind::Match
        | TokenKind::At
        | TokenKind::Hash
        | TokenKind::Backslash
        | TokenKind::Dollar
        | TokenKind::Backtick
        | TokenKind::Newline
        | TokenKind::Whitespace
        | TokenKind::DocComment
        | TokenKind::Placeholder
        | TokenKind::DecimalLiteral
        | TokenKind::Spawn
        | TokenKind::Parallel
        | TokenKind::Start
        | TokenKind::RawStr => None,
    }
}

pub fn scan_with_config(
    source: &str,
    filename: &str,
    config: crate::scanner::LexerConfig,
) -> (
    Vec<varn_core::Token>,
    std::sync::Arc<[u8]>,
    Vec<varn_core::Diagnostic>,
) {
    let (tokens, buf, diags, _) = scan_inner(source, filename, config);
    (tokens, buf, diags)
}

fn scan_inner(
    source: &str,
    _filename: &str,
    config: crate::scanner::LexerConfig,
) -> (
    Vec<varn_core::Token>,
    std::sync::Arc<[u8]>,
    Vec<varn_core::Diagnostic>,
    Vec<varn_core::Trivia>,
) {
    use varn_core::{SourceLocation, SourceRange, Token, TokenKind};

    let src_bytes = source.as_bytes();

    let mut scanner = Scanner::with_config(src_bytes, config);
    let (records, lexeme_buf) = scanner.scan_all();
    let diagnostics = scanner.diagnostics.take();
    let trivia = std::mem::take(&mut scanner.trivia);

    let tokens: Vec<varn_core::Token> = records
        .into_iter()
        .map(|r| {
            let lexeme_str = std::str::from_utf8(
                &lexeme_buf[r.lex_offset as usize..(r.lex_offset + r.lex_len) as usize],
            )
            .unwrap_or("");

            let start = SourceLocation {
                line: r.start_line,
                column: r.start_col,
                offset: r.start_offset,
            };
            let end = SourceLocation {
                line: r.end_line,
                column: r.end_col,
                offset: r.end_offset,
            };
            let range = SourceRange { start, end };

            let kind = TokenKind::from_u32(r.kind);
            let parsed_num = parse_num_lexeme(kind, lexeme_str);

            Token {
                kind,
                range,
                parsed_num,
                lex_start: r.lex_offset,
                lex_len: r.lex_len,
            }
        })
        .collect();

    let rc_buf: std::sync::Arc<[u8]> = lexeme_buf.into();
    (tokens, rc_buf, diagnostics, trivia)
}
