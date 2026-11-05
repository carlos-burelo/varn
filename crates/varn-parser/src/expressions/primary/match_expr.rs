use crate::error::{PResult, ParseError};
use crate::stream::TokenStream;
use varn_core::ast::expr::MatchCase;
use varn_core::ast::{ExprId, MatchBody, MatchField, MatchPattern, SequenceRest, VariantPayload};
use varn_core::TokenKind;

pub(super) fn parse_match_expr(s: &mut TokenStream) -> PResult<ExprId> {
    let range = s.range();
    s.advance();

    let subject = if s.eat(TokenKind::LParen) {
        let e = super::super::parse_expr(s)?;
        s.expect(TokenKind::RParen)?;
        e
    } else {
        super::super::parse_binary_expr(s, super::super::Prec::None)?
    };

    s.expect(TokenKind::LBrace)?;
    let mut cases = vec![];
    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        cases.extend(parse_match_case(s)?);
    }
    s.expect(TokenKind::RBrace)?;
    let full_range = s.span_from(range);
    Ok(s.expr(
        full_range,
        varn_core::ast::ExprKind::Match { subject, cases },
    ))
}

fn parse_match_case(s: &mut TokenStream) -> PResult<Vec<MatchCase>> {
    let range = s.range();

    let mut patterns = vec![parse_match_pattern(s)?];
    while s.eat(TokenKind::Pipe) {
        patterns.push(parse_match_pattern(s)?);
    }

    let guard = if s.eat(TokenKind::If) {
        Some(super::super::parse_conditional_expr(s)?)
    } else {
        None
    };
    s.expect(TokenKind::FatArrow)?;
    let body = if s.check(TokenKind::LBrace) {
        MatchBody::Block(crate::parser::parse_block(s)?)
    } else {
        MatchBody::Expr(super::super::parse_expr(s)?)
    };
    s.eat(TokenKind::Comma);

    let full_case_range = s.span_from(range);
    let cases = patterns
        .into_iter()
        .map(|pattern| MatchCase {
            pattern,
            guard,
            body: body.clone(),
            range: full_case_range,
        })
        .collect();
    Ok(cases)
}

fn parse_match_pattern(s: &mut TokenStream) -> PResult<MatchPattern> {
    match s.kind() {
        TokenKind::Placeholder => {
            s.advance();
            Ok(MatchPattern::Wildcard)
        }
        TokenKind::Identifier => parse_identifier_match_pattern(s),
        TokenKind::LParen => parse_tuple_match_pattern(s),
        TokenKind::LBracket => parse_array_match_pattern(s),
        TokenKind::LBrace => {
            let (fields, rest) = parse_match_fields(s)?;
            Ok(MatchPattern::Record { fields, rest })
        }
        TokenKind::EOF
        | TokenKind::Dynamic
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
        | TokenKind::RParen
        | TokenKind::RBrace
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
        | TokenKind::DecimalLiteral
        | TokenKind::Spawn
        | TokenKind::Parallel
        | TokenKind::Start
        | TokenKind::RawStr => {
            let expr = super::parse_primary_expr(s)?;
            Ok(MatchPattern::Literal(expr))
        }
    }
}

fn parse_identifier_match_pattern(s: &mut TokenStream) -> PResult<MatchPattern> {
    let id_range = s.range();
    let name = s.consume_lexeme();
    if s.check(TokenKind::LParen) {
        let payload =
            VariantPayload::Positional(parse_match_list(s, TokenKind::LParen, TokenKind::RParen)?);
        return Ok(MatchPattern::EnumVariant {
            enum_name: name,
            variant_name: name,
            payload,
        });
    }
    if s.check(TokenKind::LBrace) {
        let (fields, rest) = parse_match_fields(s)?;
        return Ok(MatchPattern::EnumVariant {
            enum_name: name,
            variant_name: name,
            payload: VariantPayload::Named { fields, rest },
        });
    }

    if s.check(TokenKind::Dot) {
        let id_expr = s.expr(id_range, varn_core::ast::ExprKind::Identifier { name });
        s.advance();
        let prop_name = s.consume_lexeme();
        let prop_range = s.prev_range();
        let prop_expr = s.expr(
            prop_range,
            varn_core::ast::ExprKind::Identifier { name: prop_name },
        );
        let expr = s.expr(
            id_range.to(prop_range),
            varn_core::ast::ExprKind::Member {
                object: id_expr,
                property: prop_expr,
                computed: false,
                optional: false,
            },
        );
        return Ok(MatchPattern::Literal(expr));
    }
    Ok(MatchPattern::Identifier(name))
}

fn parse_match_list(
    s: &mut TokenStream,
    open: TokenKind,
    close: TokenKind,
) -> PResult<Vec<MatchPattern>> {
    s.expect(open)?;
    let mut items = Vec::new();
    while !s.check(close) && !s.is_eof() {
        items.push(parse_match_pattern(s)?);
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    s.expect(close)?;
    Ok(items)
}

fn parse_tuple_match_pattern(s: &mut TokenStream) -> PResult<MatchPattern> {
    s.expect(TokenKind::LParen)?;
    let mut items = Vec::new();
    let mut trailing_comma = false;
    while !s.check(TokenKind::RParen) && !s.is_eof() {
        items.push(parse_match_pattern(s)?);
        trailing_comma = s.eat(TokenKind::Comma);
        if !trailing_comma {
            break;
        }
    }
    s.expect(TokenKind::RParen)?;
    if items.len() == 1 && !trailing_comma {
        return Ok(items.remove(0));
    }
    Ok(MatchPattern::Tuple(items))
}

fn parse_array_match_pattern(s: &mut TokenStream) -> PResult<MatchPattern> {
    s.expect(TokenKind::LBracket)?;
    let mut elements = Vec::new();
    let mut rest = SequenceRest::Closed;
    while !s.check(TokenKind::RBracket) && !s.is_eof() {
        if s.eat(TokenKind::DotDotDot) {
            rest = if s.check(TokenKind::Identifier) {
                SequenceRest::Bind(s.consume_lexeme())
            } else {
                s.eat(TokenKind::Placeholder);
                SequenceRest::Open
            };
            s.eat(TokenKind::Comma);
            if !s.check(TokenKind::RBracket) {
                return Err(ParseError::invalid(
                    "a rest pattern must be the last element of an array pattern",
                ));
            }
            break;
        }
        elements.push(parse_match_pattern(s)?);
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    s.expect(TokenKind::RBracket)?;
    Ok(MatchPattern::Array { elements, rest })
}

fn parse_match_fields(s: &mut TokenStream) -> PResult<(Vec<MatchField>, bool)> {
    s.expect(TokenKind::LBrace)?;
    let mut fields: Vec<MatchField> = Vec::new();
    let mut rest = false;
    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        if s.eat(TokenKind::DotDotDot) {
            rest = true;
            s.eat(TokenKind::Comma);
            if !s.check(TokenKind::RBrace) {
                return Err(ParseError::invalid("`...` must be the last entry of a record pattern".to_owned()));
            }
            break;
        }
        let range = s.range();
        let name = s.expect_id()?;
        if fields.iter().any(|f| f.name == name) {
            return Err(ParseError::invalid(format!(
                "the pattern already names the field '{}'",
                s.interner.resolve(name)
            )));
        }
        let pattern = if s.eat(TokenKind::Colon) {
            Some(parse_match_pattern(s)?)
        } else {
            None
        };
        fields.push(MatchField {
            name,
            pattern,
            range,
        });
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    s.expect(TokenKind::RBrace)?;
    Ok((fields, rest))
}
