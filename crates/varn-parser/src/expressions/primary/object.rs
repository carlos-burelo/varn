use crate::stream::TokenStream;
use crate::types::parse_type;
use varn_core::ast::expr::PropKey;
use varn_core::ast::{ExprId, ObjectProp};
use varn_core::TokenKind;

pub(super) fn parse_object_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    let range = s.range();
    s.advance();
    let properties = parse_object_body(s)?;
    s.expect(TokenKind::RBrace)?;
    let full_range = s.span_from(range);
    Ok(s.expr(full_range, varn_core::ast::ExprKind::Object { properties }))
}

pub(crate) fn parse_object_body(s: &mut TokenStream) -> Result<Vec<ObjectProp>, String> {
    let mut properties = vec![];

    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        let prop_range = s.range();

        if s.check(TokenKind::DotDotDot) {
            s.advance();
            let arg = super::super::parse_assign_expr(s)?;
            let full_prop_range = s.span_from(prop_range);
            properties.push(ObjectProp::Spread {
                argument: arg,
                range: full_prop_range,
            });
            s.eat(TokenKind::Comma);
            continue;
        }

        let is_getter = s.check(TokenKind::Get)
            && s.peek_kind(1) != TokenKind::LParen
            && s.peek_kind(1) != TokenKind::Colon
            && s.peek_kind(1) != TokenKind::Comma
            && s.peek_kind(1) != TokenKind::RBrace;
        let is_setter = s.check(TokenKind::Set)
            && s.peek_kind(1) != TokenKind::LParen
            && s.peek_kind(1) != TokenKind::Colon
            && s.peek_kind(1) != TokenKind::Comma
            && s.peek_kind(1) != TokenKind::RBrace;

        if is_getter {
            s.advance();
            let key = parse_prop_key(s)?;
            s.expect(TokenKind::LParen)?;
            s.expect(TokenKind::RParen)?;
            let return_type = if s.eat(TokenKind::Colon) {
                Some(parse_type(s)?)
            } else {
                None
            };
            let body = crate::parser::parse_block(s)?;
            let full_prop_range = s.span_from(prop_range);
            properties.push(ObjectProp::Getter {
                key,
                body,
                return_type,
                range: full_prop_range,
            });
            s.eat(TokenKind::Comma);
            continue;
        }

        if is_setter {
            s.advance();
            let key = parse_prop_key(s)?;
            s.expect(TokenKind::LParen)?;
            let param = crate::parser::parse_single_param(s)?;
            s.expect(TokenKind::RParen)?;
            let body = crate::parser::parse_block(s)?;
            let full_prop_range = s.span_from(prop_range);
            properties.push(ObjectProp::Setter {
                key,
                param,
                body,
                range: full_prop_range,
            });
            s.eat(TokenKind::Comma);
            continue;
        }

        let is_async = s.check(TokenKind::Async)
            && s.peek_kind(1) != TokenKind::Colon
            && s.peek_kind(1) != TokenKind::Comma
            && s.peek_kind(1) != TokenKind::RBrace
            && s.peek_kind(1) != TokenKind::LParen;
        if is_async {
            s.advance();
        }
        let is_generator = s.eat(TokenKind::Star);
        let key = parse_prop_key(s)?;

        if s.check(TokenKind::LParen) {
            let params = crate::parser::parse_params(s)?;
            let return_type = if s.eat(TokenKind::Colon) {
                Some(parse_type(s)?)
            } else {
                None
            };
            let body = crate::parser::parse_block(s)?;
            let full_prop_range = s.span_from(prop_range);
            properties.push(ObjectProp::Method {
                key,
                params,
                body,
                return_type,
                is_async,
                is_generator,
                range: full_prop_range,
            });
            s.eat(TokenKind::Comma);
            continue;
        }

        if is_generator {
            return Err(String::from("unexpected `*` before property"));
        }
        if is_async {
            return Err(String::from("unexpected `async` before property"));
        }

        let shorthand = !s.check(TokenKind::Colon);
        let value = if s.eat(TokenKind::Colon) {
            super::super::parse_assign_expr(s)?
        } else {
            let name = match &key {
                PropKey::Identifier(n) => s.interner.intern(n),
                PropKey::Str(_) | PropKey::Int(_) | PropKey::Computed(_) => {
                    return Err(String::from("shorthand property must be an identifier"))
                }
            };
            s.expr(prop_range, varn_core::ast::ExprKind::Identifier { name })
        };

        let computed = matches!(&key, PropKey::Computed(_));
        let full_prop_range = s.span_from(prop_range);
        properties.push(ObjectProp::Property {
            key,
            value,
            shorthand,
            computed,
            range: full_prop_range,
        });
        s.eat(TokenKind::Comma);
    }

    Ok(properties)
}

fn parse_prop_key(s: &mut TokenStream) -> Result<PropKey, String> {
    match s.kind() {
        TokenKind::Identifier => {
            let atom = s.consume_lexeme();
            Ok(PropKey::Identifier(s.interner.resolve(atom).to_string()))
        }
        TokenKind::Str => {
            let atom = s.consume_lexeme();
            Ok(PropKey::Str(s.interner.resolve(atom).to_string()))
        }
        TokenKind::IntegerLiteral => {
            let pre_parsed = s.parsed_num();
            let raw = s.consume_lexeme();
            let raw_text = s.interner.resolve(raw);
            let v = match pre_parsed {
                Some(varn_core::ParsedNumber::Int(n)) => n,
                Some(varn_core::ParsedNumber::Float(_)) | None => {
                    super::super::literal_text::parse_int_radix(raw_text).unwrap_or(0)
                }
            };
            Ok(PropKey::Int(v))
        }
        TokenKind::LBracket => {
            s.advance();
            let expr = super::super::parse_assign_expr(s)?;
            s.expect(TokenKind::RBracket)?;
            Ok(PropKey::Computed(expr))
        }
        _ if s.kind().can_be_identifier() || s.kind().is_keyword() => {
            let atom = s.consume_lexeme();
            Ok(PropKey::Identifier(s.interner.resolve(atom).to_string()))
        }
        TokenKind::EOF
        | TokenKind::Dynamic
        | TokenKind::FloatLiteral
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
        | TokenKind::RawStr => Err(format!("Expected property key, got {:?}", s.kind())),
    }
}
