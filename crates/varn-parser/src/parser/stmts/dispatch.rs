use super::abrupt::{
    parse_break_stmt, parse_continue_stmt, parse_return_stmt, parse_throw_stmt, parse_try_stmt,
    parse_using_stmt,
};
use super::block::parse_block;
use super::branches::{
    parse_do_while_stmt, parse_for_stmt, parse_if_stmt, parse_switch_stmt, parse_while_stmt,
};
use crate::stream::TokenStream;
use varn_core::ast::{StmtId, StmtKind};
use varn_core::TokenKind;

pub fn parse_stmt_or_decl_inner(s: &mut TokenStream) -> Result<StmtId, String> {
    while s.check(TokenKind::DocComment) {
        let lexeme: std::sync::Arc<str> = std::sync::Arc::from(s.lexeme());
        s.advance();
        s.store_pending_doc(lexeme);
    }

    let decorators = if s.check(TokenKind::At) {
        super::super::patterns::parse_decorator_list(s)?
    } else {
        Vec::new()
    };
    let kind = s.kind();
    let next_kind = s.peek_kind(1);

    let has_decorators = !decorators.is_empty();
    if let Some(decl_stmt) =
        super::super::stmt_decls::try_parse_decl_stmt(s, kind, next_kind, decorators)
    {
        return decl_stmt;
    }
    if has_decorators {
        return Err("expected a declaration after decorators".to_owned());
    }

    let _ = s.current_doc();

    match kind {
        TokenKind::LBrace => parse_block(s),
        TokenKind::Semicolon => {
            let range = s.range();
            s.advance();
            Ok(s.stmt(range, StmtKind::Empty))
        }
        TokenKind::If => parse_if_stmt(s),
        TokenKind::While => parse_while_stmt(s),
        TokenKind::Do => parse_do_while_stmt(s),
        TokenKind::For => parse_for_stmt(s),
        TokenKind::Switch => parse_switch_stmt(s),
        TokenKind::Return => parse_return_stmt(s),
        TokenKind::Break => parse_break_stmt(s),
        TokenKind::Continue => parse_continue_stmt(s),
        TokenKind::Throw => parse_throw_stmt(s),
        TokenKind::Try if s.peek_kind(1) == TokenKind::LBrace => parse_try_stmt(s),
        TokenKind::Using => parse_using_stmt(s, false),
        TokenKind::Await if next_kind == TokenKind::Using => parse_using_stmt(s, true),
        TokenKind::With => Err(String::from(
            "`with` is not supported; use explicit variable bindings",
        )),

        TokenKind::Identifier if next_kind == TokenKind::Colon => {
            let start_range = s.range();
            let label = s.consume_lexeme();
            s.advance();
            let body = parse_stmt_or_decl_inner(s)?;
            let range = s.span_from(start_range);
            Ok(s.stmt(range, StmtKind::Labeled { label, body }))
        }

        TokenKind::EOF
        | TokenKind::Dynamic
        | TokenKind::Identifier
        | TokenKind::IntegerLiteral
        | TokenKind::FloatLiteral
        | TokenKind::BinaryLiteral
        | TokenKind::OctalLiteral
        | TokenKind::HexLiteral
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
        | TokenKind::RBrace
        | TokenKind::LBracket
        | TokenKind::RBracket
        | TokenKind::LAngle
        | TokenKind::RAngle
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
        | TokenKind::Else
        | TokenKind::Case
        | TokenKind::Default
        | TokenKind::Try
        | TokenKind::Catch
        | TokenKind::Finally
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
        | TokenKind::RawStr => {
            let start_range = s.range();
            let expr = crate::expressions::parse_seq_expr(s)?;
            s.eat_semicolon();
            let range = s.span_from(start_range);
            Ok(s.stmt(range, StmtKind::Expr { expression: expr }))
        }
    }
}
