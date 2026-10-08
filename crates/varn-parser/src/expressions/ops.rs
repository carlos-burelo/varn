use super::parse_assign_expr;
use crate::stream::TokenStream;
use crate::types::parse_type;
use varn_core::ast::expr::ArrowBody;
use varn_core::ast::{ExprId, ExprKind, Param, Pattern};
use varn_core::TokenKind;

pub(super) fn could_be_arrow(s: &TokenStream) -> bool {
    let k = s.kind();
    if k == TokenKind::LParen {
        return paren_leads_to_arrow(s);
    }
    if k == TokenKind::Identifier && s.peek_kind(1) == TokenKind::FatArrow {
        return true;
    }
    if k == TokenKind::Async {
        let k2 = s.peek_kind(1);
        if k2 == TokenKind::Identifier || k2 == TokenKind::LParen {
            return true;
        }
    }
    false
}

fn paren_leads_to_arrow(s: &TokenStream) -> bool {
    let mut depth = 0i32;
    let mut off = 0usize;
    loop {
        match s.peek_kind(off) {
            TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => depth += 1,
            TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                depth -= 1;
                if depth == 0 {
                    let mut next_off = off + 1;
                    if s.peek_kind(next_off) == TokenKind::Colon {
                        next_off += 1;
                        let mut type_depth = 0i32;
                        loop {
                            match s.peek_kind(next_off) {
                                TokenKind::LAngle | TokenKind::LBracket | TokenKind::LParen => {
                                    type_depth += 1;
                                }
                                TokenKind::RAngle | TokenKind::RBracket | TokenKind::RParen => {
                                    type_depth -= 1;
                                }
                                TokenKind::FatArrow if type_depth == 0 => return true,
                                TokenKind::EOF
                                | TokenKind::Semicolon
                                | TokenKind::LBrace
                                | TokenKind::RBrace => return false,
                                TokenKind::Dynamic
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
                                | TokenKind::RawStr => {}
                            }
                            next_off += 1;
                            if next_off > 128 {
                                return true;
                            }
                        }
                    }
                    return s.peek_kind(next_off) == TokenKind::FatArrow;
                }
            }
            TokenKind::EOF | TokenKind::Semicolon => return false,
            TokenKind::Dynamic
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
            | TokenKind::RawStr => {}
        }
        off += 1;
        if off > 128 {
            return true;
        }
    }
}

pub(super) fn try_parse_arrow(s: &mut TokenStream) -> Result<Option<ExprId>, String> {
    let save = s.save();
    match parse_arrow_attempt(s) {
        Ok(expr) => Ok(Some(expr)),
        Err(_) => {
            s.restore(save);
            Ok(None)
        }
    }
}

fn parse_arrow_attempt(s: &mut TokenStream) -> Result<ExprId, String> {
    let start_range = s.range();
    let is_async = s.eat(TokenKind::Async);

    let params = if s.check(TokenKind::LParen) {
        crate::parser::parse_params(s)?
    } else {
        let param_text = s.lexeme().to_owned();
        let param_name = s.interner.intern(&param_text);
        let tok = s.expect_token(TokenKind::Identifier)?;
        let param_range = tok.range;
        vec![Param {
            pattern: Pattern::Identifier {
                name: param_name,
                range: param_range,
            },
            type_ann: None,
            default: None,
            is_rest: false,
            is_optional: false,
            modifiers: Default::default(),
            range: param_range,
        }]
    };

    let return_type = if s.eat(TokenKind::Colon) {
        Some(parse_type(s)?)
    } else {
        None
    };
    s.expect(TokenKind::FatArrow)?;

    let body = if s.check(TokenKind::LBrace) {
        ArrowBody::Block(crate::parser::parse_block(s)?)
    } else {
        ArrowBody::Expr(parse_assign_expr(s)?)
    };

    let full_range = s.span_from(start_range);
    Ok(s.expr(
        full_range,
        ExprKind::Arrow {
            params,
            return_type,
            body: Box::new(body),
            is_async,
        },
    ))
}

pub(super) fn parse_yield_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    let start_range = s.range();
    s.advance();
    let delegate = s.eat(TokenKind::Star);
    let argument = if !s.check(TokenKind::Semicolon) && !s.check(TokenKind::RBrace) && !s.is_eof() {
        Some(parse_assign_expr(s)?)
    } else {
        None
    };
    let full_range = s.span_from(start_range);
    Ok(s.expr(full_range, ExprKind::Yield { argument, delegate }))
}
