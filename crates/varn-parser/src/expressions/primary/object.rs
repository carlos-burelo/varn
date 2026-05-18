use crate::stream::TokenStream;
use crate::types::parse_type;
use varn_core::ast::expr::PropKey;
use varn_core::ast::{Expr, ObjectProp};
use varn_core::TokenKind;

pub(super) fn parse_object_expr(s: &mut TokenStream) -> Result<Expr, String> {
    let range = s.range();
    s.advance();
    let mut properties = vec![];

    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        let prop_range = s.range();

        if s.check(TokenKind::DotDotDot) {
            s.advance();
            let arg = super::super::parse_assign_expr(s)?;
            properties.push(ObjectProp::Spread {
                argument: arg,
                range: prop_range,
            });
            s.eat(TokenKind::Comma);
            continue;
        }

        let is_getter = s.check(TokenKind::Get)
            && s.peek_kind(1) != TokenKind::LParen
            && s.peek_kind(1) != TokenKind::Colon;
        let is_setter = s.check(TokenKind::Set)
            && s.peek_kind(1) != TokenKind::LParen
            && s.peek_kind(1) != TokenKind::Colon;

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
            properties.push(ObjectProp::Getter {
                key,
                body: Box::new(body),
                return_type,
                range: prop_range,
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
            properties.push(ObjectProp::Setter {
                key,
                param,
                body: Box::new(body),
                range: prop_range,
            });
            s.eat(TokenKind::Comma);
            continue;
        }

        let is_async = s.eat(TokenKind::Async);
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
            properties.push(ObjectProp::Method {
                key,
                params,
                body: Box::new(body),
                return_type,
                is_async,
                is_generator,
                range: prop_range,
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
                PropKey::Identifier(n) => n.clone(),
                _ => return Err(String::from("shorthand property must be an identifier")),
            };
            Expr::new_with_range(
                prop_range,
                varn_core::ast::ExprKind::Identifier { name: name.into() },
            )
        };

        let computed = matches!(&key, PropKey::Computed(_));
        properties.push(ObjectProp::Property {
            key,
            value,
            shorthand,
            computed,
            range: prop_range,
        });
        s.eat(TokenKind::Comma);
    }

    s.expect(TokenKind::RBrace)?;
    Ok(Expr::new_with_range(
        range,
        varn_core::ast::ExprKind::Object { properties },
    ))
}

fn parse_prop_key(s: &mut TokenStream) -> Result<PropKey, String> {
    match s.kind() {
        TokenKind::Identifier => Ok(PropKey::Identifier(s.consume_lexeme().to_string())),
        TokenKind::Str => Ok(PropKey::Str(s.consume_lexeme().to_string())),
        TokenKind::IntegerLiteral => {
            let raw = s.consume_lexeme();
            Ok(PropKey::Int(
                super::super::helpers::parse_int_radix(&raw).unwrap_or(0),
            ))
        }
        TokenKind::LBracket => {
            s.advance();
            let expr = super::super::parse_assign_expr(s)?;
            s.expect(TokenKind::RBracket)?;
            Ok(PropKey::Computed(expr))
        }
        _ if s.kind().can_be_identifier() || s.kind().is_keyword() => {
            Ok(PropKey::Identifier(s.consume_lexeme().to_string()))
        }
        _ => Err(format!("Expected property key, got {:?}", s.kind())),
    }
}
