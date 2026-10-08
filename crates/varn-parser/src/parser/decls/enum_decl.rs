use crate::expressions::parse_expr;
use crate::stream::TokenStream;
use crate::types::{parse_type, parse_type_params};
use varn_core::ast::{EnumDecl, EnumField, EnumMember};
use varn_core::TokenKind;

pub fn parse_enum_decl(s: &mut TokenStream) -> Result<EnumDecl, String> {
    let range = s.range();
    s.expect(TokenKind::Enum)?;
    let id = s.expect_id()?;

    let type_params = if s.check(TokenKind::LAngle) {
        parse_type_params(s)?
    } else {
        vec![]
    };

    let implements = if s.eat(TokenKind::Implements) {
        let mut impls = vec![parse_type(s)?];
        while s.eat(TokenKind::Comma) {
            impls.push(parse_type(s)?);
        }
        impls
    } else {
        vec![]
    };

    s.expect(TokenKind::LBrace)?;
    let mut members = vec![];
    let mut body = vec![];

    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        if s.eat(TokenKind::Semicolon) {
            break;
        }
        if s.check(TokenKind::At) {
            break;
        }

        let mem_range = s.range();
        let name = s.expect_id()?;

        let mut payload_fields: Vec<EnumField> = Vec::new();
        if s.check(TokenKind::LParen) {
            s.advance();
            let mut idx = 0;
            while !s.check(TokenKind::RParen) && !s.is_eof() {
                let field_range = s.range();
                let field_name =
                    if s.check(TokenKind::Identifier) && s.peek_kind(1) == TokenKind::Colon {
                        let name = s.consume_lexeme();
                        s.expect(TokenKind::Colon)?;
                        name
                    } else {
                        let n = s.interner.intern(&format!("value{idx}"));
                        idx += 1;
                        n
                    };
                let (ty, init) = match s.kind() {
                    TokenKind::IntegerLiteral => {
                        let expr = crate::expressions::parse_expr(s)?;
                        (
                            s.type_node(
                                field_range,
                                varn_core::TypeKind::Primitive(varn_core::LangPrimitive::Int),
                            ),
                            Some(expr),
                        )
                    }
                    TokenKind::FloatLiteral => {
                        let expr = crate::expressions::parse_expr(s)?;
                        (
                            s.type_node(
                                field_range,
                                varn_core::TypeKind::Primitive(varn_core::LangPrimitive::Float),
                            ),
                            Some(expr),
                        )
                    }
                    TokenKind::Str => {
                        let expr = crate::expressions::parse_expr(s)?;
                        (
                            s.type_node(
                                field_range,
                                varn_core::TypeKind::Primitive(varn_core::LangPrimitive::Str),
                            ),
                            Some(expr),
                        )
                    }
                    TokenKind::True | TokenKind::False => {
                        let expr = crate::expressions::parse_expr(s)?;
                        (
                            s.type_node(
                                field_range,
                                varn_core::TypeKind::Primitive(varn_core::LangPrimitive::Bool),
                            ),
                            Some(expr),
                        )
                    }
                    TokenKind::EOF
                    | TokenKind::Dynamic
                    | TokenKind::Identifier
                    | TokenKind::BinaryLiteral
                    | TokenKind::OctalLiteral
                    | TokenKind::HexLiteral
                    | TokenKind::BigIntLiteral
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
                        let parsed_ty = crate::types::parse_type(s)?;
                        (parsed_ty, None)
                    }
                };

                let f_full_range = s.span_from(field_range);
                payload_fields.push(EnumField {
                    name: field_name,
                    ty,
                    init,
                    range: f_full_range,
                });
                if s.check(TokenKind::Comma) {
                    s.advance();
                }
            }
            s.expect(TokenKind::RParen)?;
        }

        let init = if payload_fields.is_empty() && s.eat(TokenKind::Eq) {
            Some(parse_expr(s)?)
        } else {
            None
        };

        let full_mem_range = s.span_from(mem_range);
        members.push(EnumMember {
            id: name,
            init,
            payload_fields,
            range: full_mem_range,
        });

        let has_comma = s.eat(TokenKind::Comma);
        if s.check(TokenKind::Semicolon) {
            continue;
        }
        if !has_comma && !s.check(TokenKind::Identifier) && !s.check(TokenKind::Semicolon) {
            break;
        }
    }

    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        while s.eat(TokenKind::Semicolon) {}
        while s.check(TokenKind::DocComment) {
            s.advance();
        }
        if s.check(TokenKind::RBrace) {
            break;
        }
        body.push(super::class_member::parse_class_member(s, false)?);
    }

    s.expect(TokenKind::RBrace)?;
    let full_range = s.span_from(range);
    Ok(EnumDecl {
        id,
        ast_id: s.next_ast_id(),
        type_params,
        implements,
        members,
        body,
        doc: None,
        range: full_range,
    })
}
