use crate::stream::TokenStream;
use varn_core::ast::{TypeNode, TypeParam};
use varn_core::{TokenKind, TypeKind};

pub(crate) fn parse_primary_type(s: &mut TokenStream) -> Result<TypeNode, String> {
    let range = s.range();

    match s.kind() {
        TokenKind::Template | TokenKind::TemplateHead => {
            super::composite::parse_template_literal_type(s)
        }

        TokenKind::Identifier => {
            if s.lexeme() == "keyof" {
                s.advance();
                let inner = super::suffixes::parse_array_type(s)?;
                let full_range = s.span_from(range);
                return Ok(s.type_node(full_range, TypeKind::KeyOf(Box::new(inner))));
            }

            if s.lexeme() == "infer" && s.peek_kind(1) == TokenKind::Identifier {
                s.advance();
                let name = s.consume_lexeme();
                let full_range = s.span_from(range);
                return Ok(s.type_node(full_range, TypeKind::Infer(name)));
            }

            let mut name_buf = s.lexeme().to_owned();
            s.advance();

            while s.check(TokenKind::Dot) {
                s.advance();
                if s.check(TokenKind::Identifier) {
                    name_buf.push('.');
                    name_buf.push_str(s.lexeme());
                    s.advance();
                } else {
                    break;
                }
            }
            if name_buf == "Unit" && !s.check(TokenKind::LAngle) {
                let full_range = s.span_from(range);
                return Ok(s.type_node(full_range, TypeKind::Tuple(vec![])));
            }
            let name = s.interner.intern(&name_buf);

            let type_args = if s.check(TokenKind::LAngle) {
                super::generics::parse_type_args(s)?
            } else {
                vec![]
            };
            let full_range = s.span_from(range);
            let kind = if type_args.is_empty() {
                TypeKind::Named(name, None)
            } else {
                TypeKind::Generic(name, type_args, None)
            };
            Ok(s.type_node(full_range, kind))
        }

        TokenKind::This => {
            s.advance();
            Ok(s.type_node(range, TypeKind::This))
        }

        TokenKind::Typeof => {
            s.advance();
            let expr = crate::expressions::parse_unary_expr(s)?;
            let full_range = s.span_from(range);
            Ok(s.type_node(full_range, TypeKind::Typeof(expr)))
        }

        TokenKind::LParen => {
            s.advance();

            if s.check(TokenKind::RParen) {
                s.advance();
                if s.eat(TokenKind::FatArrow) {
                    let ret = super::entry::parse_type(s)?;
                    let full_range = s.span_from(range);
                    return Ok(s.type_node(full_range, TypeKind::Fn((vec![], Box::new(ret)))));
                }
                let full_range = s.span_from(range);
                return Ok(s.type_node(full_range, TypeKind::Tuple(vec![])));
            }

            if (s.kind() == TokenKind::Identifier && s.peek_kind(1) == TokenKind::Colon)
                || s.check(TokenKind::DotDotDot)
            {
                let params = super::generics::parse_fn_type_params(s)?;
                s.expect(TokenKind::RParen)?;
                s.expect(TokenKind::FatArrow)?;
                let ret = super::entry::parse_type(s)?;
                let full_range = s.span_from(range);
                return Ok(s.type_node(full_range, TypeKind::Fn((params, Box::new(ret)))));
            }

            let first = super::entry::parse_type(s)?;
            if s.eat(TokenKind::Comma) {
                let mut param_types = vec![first];
                while !s.check(TokenKind::RParen) && !s.is_eof() {
                    param_types.push(super::entry::parse_type(s)?);
                    if !s.eat(TokenKind::Comma) {
                        break;
                    }
                }
                s.expect(TokenKind::RParen)?;
                s.expect(TokenKind::FatArrow)?;
                let ret = super::entry::parse_type(s)?;
                let full_range = s.span_from(range);
                let params = param_types
                    .into_iter()
                    .map(|ty| {
                        let ty_range = *ty.range();
                        TypeParam {
                            name: s.interner.intern("_"),
                            constraint: Some(ty),
                            default: None,
                            range: ty_range,
                        }
                    })
                    .collect();
                return Ok(s.type_node(full_range, TypeKind::Fn((params, Box::new(ret)))));
            }

            s.expect(TokenKind::RParen)?;
            if s.eat(TokenKind::FatArrow) {
                let ret = super::entry::parse_type(s)?;
                let full_range = s.span_from(range);
                let first_range = *first.range();
                let underscore = s.interner.intern("_");
                return Ok(s.type_node(
                    full_range,
                    TypeKind::Fn((
                        vec![TypeParam {
                            name: underscore,
                            constraint: Some(first),
                            default: None,
                            range: first_range,
                        }],
                        Box::new(ret),
                    )),
                ));
            }

            Ok(first)
        }

        TokenKind::Hash if s.peek_kind(1) == TokenKind::LBracket => {
            s.advance();
            super::composite::parse_tuple_type(s, range)
        }
        TokenKind::LBracket => super::composite::parse_tuple_type(s, range),

        TokenKind::LBrace => {
            s.advance();

            let is_mapped = if s.check(TokenKind::Readonly) {
                s.peek_kind(1) == TokenKind::LBracket
                    && s.peek_kind(2) == TokenKind::Identifier
                    && s.peek_kind(3) == TokenKind::In
            } else {
                s.check(TokenKind::LBracket)
                    && s.peek_kind(1) == TokenKind::Identifier
                    && s.peek_kind(2) == TokenKind::In
            };

            if is_mapped {
                let mapped_readonly = s.eat(TokenKind::Readonly);
                s.advance();
                let key_var = s.consume_lexeme();
                s.advance();
                let source = super::entry::parse_type(s)?;
                s.expect(TokenKind::RBracket)?;
                let optional = s.eat(TokenKind::Question);
                s.expect(TokenKind::Colon)?;
                let value = super::entry::parse_type(s)?;
                s.expect(TokenKind::RBrace)?;
                let full_range = s.span_from(range);
                return Ok(s.type_node(
                    full_range,
                    TypeKind::Mapped {
                        key_var,
                        source: Box::new(source),
                        value: Box::new(value),
                        optional,
                        readonly: mapped_readonly,
                    },
                ));
            }

            let mut members = vec![];
            while !s.check(TokenKind::RBrace) && !s.is_eof() {
                while s.eat(TokenKind::Semicolon) || s.eat(TokenKind::Comma) {}
                if s.check(TokenKind::RBrace) {
                    break;
                }
                members.push(crate::parser::decls::type_decls::parse_interface_member(s)?);

                s.eat(TokenKind::Comma);
                s.eat(TokenKind::Semicolon);
            }
            s.expect(TokenKind::RBrace)?;
            let full_range = s.span_from(range);
            Ok(s.type_node(full_range, TypeKind::Object(members)))
        }

        TokenKind::Str
        | TokenKind::RawStr
        | TokenKind::Char
        | TokenKind::IntegerLiteral
        | TokenKind::BinaryLiteral
        | TokenKind::OctalLiteral
        | TokenKind::HexLiteral
        | TokenKind::Minus
        | TokenKind::True
        | TokenKind::False => super::literals::parse_literal_type(s, range),
        TokenKind::Null => {
            s.advance();
            Ok(s.type_node(range, TypeKind::Primitive(varn_core::LangPrimitive::Null)))
        }

        TokenKind::Void => {
            s.advance();
            Ok(s.type_node(range, TypeKind::Primitive(varn_core::LangPrimitive::Void)))
        }

        TokenKind::Is
        | TokenKind::On
        | TokenKind::Get
        | TokenKind::Set
        | TokenKind::From
        | TokenKind::Of
        | TokenKind::Async
        | TokenKind::Static
        | TokenKind::Abstract
        | TokenKind::Readonly
        | TokenKind::Native
        | TokenKind::Constructor
        | TokenKind::Destructor => {
            let name = s.consume_lexeme();
            Ok(s.type_node(range, TypeKind::Named(name, None)))
        }

        TokenKind::EOF | TokenKind::Dynamic | TokenKind::FloatLiteral | TokenKind::BigIntLiteral | TokenKind::TemplateMiddle | TokenKind::TemplateTail | TokenKind::RegularExpression | TokenKind::RParen | TokenKind::RBrace | TokenKind::RBracket | TokenKind::LAngle | TokenKind::RAngle | TokenKind::Semicolon | TokenKind::Comma | TokenKind::Dot | TokenKind::DotDot | TokenKind::DotDotDot | TokenKind::DotDotEq | TokenKind::Colon | TokenKind::ColonColon | TokenKind::Question | TokenKind::QuestionDot | TokenKind::QuestionLBracket | TokenKind::QuestionQuestion | TokenKind::QuestionQuestionEq | TokenKind::Plus | TokenKind::PlusPlus | TokenKind::PlusEq | TokenKind::MinusMinus | TokenKind::MinusEq | TokenKind::Star | TokenKind::StarStar | TokenKind::StarEq | TokenKind::StarStarEq | TokenKind::Slash | TokenKind::SlashEq | TokenKind::Percent | TokenKind::PercentEq | TokenKind::Amp | TokenKind::AmpAmp | TokenKind::AmpEq | TokenKind::AmpAmpEq | TokenKind::Pipe | TokenKind::PipePipe | TokenKind::PipeEq | TokenKind::PipePipeEq | TokenKind::PipeGt | TokenKind::Caret | TokenKind::CaretEq | TokenKind::Tilde | TokenKind::LtLt | TokenKind::LtLtEq | TokenKind::GtGt | TokenKind::GtGtEq | TokenKind::GtGtGt | TokenKind::GtGtGtEq | TokenKind::Eq | TokenKind::EqEq | TokenKind::EqEqEq | TokenKind::Bang | TokenKind::BangEq | TokenKind::BangEqEq | TokenKind::Lt | TokenKind::LtEq | TokenKind::Gt | TokenKind::GtEq | TokenKind::Arrow | TokenKind::FatArrow | TokenKind::Let | TokenKind::Const | TokenKind::Var | TokenKind::Function | TokenKind::Class | TokenKind::Struct | TokenKind::Interface | TokenKind::Type | TokenKind::Enum | TokenKind::Namespace | TokenKind::Module | TokenKind::Extension | TokenKind::If | TokenKind::Else | TokenKind::Switch | TokenKind::Case | TokenKind::Default | TokenKind::While | TokenKind::For | TokenKind::Do | TokenKind::Break | TokenKind::Continue | TokenKind::Return | TokenKind::Throw | TokenKind::Try | TokenKind::Catch | TokenKind::Finally | TokenKind::Using | TokenKind::With | TokenKind::Import | TokenKind::Export | TokenKind::As | TokenKind::Await | TokenKind::Yield | TokenKind::New | TokenKind::Super | TokenKind::Delete | TokenKind::Instanceof | TokenKind::In | TokenKind::Public | TokenKind::Private | TokenKind::Protected | TokenKind::Override | TokenKind::Declare | TokenKind::Extends | TokenKind::Implements | TokenKind::Match | TokenKind::At | TokenKind::Hash | TokenKind::Backslash | TokenKind::Dollar | TokenKind::Backtick | TokenKind::Newline | TokenKind::Whitespace | TokenKind::DocComment | TokenKind::Placeholder | TokenKind::DecimalLiteral | TokenKind::Spawn | TokenKind::Parallel | TokenKind::Start => Err(format!(
            "Unexpected token in type position: {:?} at {}:{}",
            s.kind(),
            s.range().start.line,
            s.range().start.column
        )),
    }
}
