use super::class_key::member_key_name;
use crate::expressions::parse_expr;
use crate::stream::TokenStream;
use crate::types::{parse_type, parse_type_params};
use varn_core::ast::ClassMember;
use varn_core::TokenKind;

pub fn member_doc(s: &mut TokenStream) -> Option<String> {
    let mut doc = None;
    while s.check(TokenKind::DocComment) {
        doc = Some(s.lexeme().to_owned());
        s.advance();
    }
    doc
}

pub fn parse_class_member(
    s: &mut TokenStream,
    class_is_declare: bool,
    doc: Option<String>,
) -> Result<ClassMember, String> {
    let range = s.range();
    let decorators = super::super::patterns::parse_decorator_list(s)?;

    let mods = super::class_mods::parse_member_mods(s);

    if mods.is_static && s.check(TokenKind::LBrace) {
        if !decorators.is_empty() {
            return Err("decorators are not supported on static blocks".to_owned());
        }
        let body = super::super::stmts::parse_block(s)?;
        let full_range = s.span_from(range);
        return Ok(ClassMember::StaticBlock {
            body,
            range: full_range,
        });
    }

    let stray = matches!(
        s.peek_kind(0),
        TokenKind::Let | TokenKind::Const | TokenKind::Var | TokenKind::Function
    ) && s.peek_kind(1) == TokenKind::Identifier;
    if stray {
        return Err(
            "a class member takes no 'let', 'const', 'var' or 'function': write 'name: Type' or 'name(...) { ... }'"
                .to_owned(),
        );
    }

    if s.check(TokenKind::Constructor) {
        s.advance();
        let params = super::super::params::parse_params(s)?;
        let body = if class_is_declare {
            if s.check(TokenKind::LBrace) {
                return Err("declare constructor cannot have a body".to_owned());
            }
            s.eat_semicolon();
            s.stmt(s.range(), varn_core::ast::StmtKind::Empty)
        } else {
            super::super::stmts::parse_block(s)?
        };
        let full_range = s.span_from(range);
        return Ok(ClassMember::Constructor {
            params,
            body,
            decorators,
            doc,
            range: full_range,
        });
    }
    if s.check(TokenKind::Destructor) {
        s.advance();
        if !decorators.is_empty() {
            return Err("decorators are not supported on destructors".to_owned());
        }
        let body = if class_is_declare {
            if s.check(TokenKind::LBrace) {
                return Err("declare destructor cannot have a body".to_owned());
            }
            s.eat_semicolon();
            s.stmt(s.range(), varn_core::ast::StmtKind::Empty)
        } else {
            super::super::stmts::parse_block(s)?
        };
        let full_range = s.span_from(range);
        return Ok(ClassMember::Destructor {
            body,
            range: full_range,
        });
    }

    let is_get = s.check(TokenKind::Get) && {
        let nk = s.peek_kind(1);
        nk != TokenKind::LParen && nk != TokenKind::Semicolon && nk != TokenKind::Colon
    };
    let is_set = s.check(TokenKind::Set) && {
        let nk = s.peek_kind(1);
        nk != TokenKind::LParen && nk != TokenKind::Semicolon && nk != TokenKind::Colon
    };

    if is_get {
        s.advance();
        let key = member_key_name(s)?;
        s.expect(TokenKind::LParen)?;
        s.expect(TokenKind::RParen)?;
        let return_type = if s.eat(TokenKind::Colon) {
            Some(parse_type(s)?)
        } else {
            None
        };
        let body = if s.check(TokenKind::LBrace) {
            if class_is_declare {
                return Err("declare getter cannot have a body".to_owned());
            }
            Some(super::super::stmts::parse_block(s)?)
        } else {
            s.eat_semicolon();
            None
        };
        let full_range = s.span_from(range);
        return Ok(ClassMember::Getter {
            key: s.interner.intern(&key),
            return_type,
            body,
            modifiers: mods,
            decorators,
            doc,
            range: full_range,
        });
    }
    if is_set {
        s.advance();
        let key = member_key_name(s)?;
        s.expect(TokenKind::LParen)?;
        let param = super::super::params::parse_single_param(s)?;
        s.expect(TokenKind::RParen)?;
        let body = if s.check(TokenKind::LBrace) {
            if class_is_declare {
                return Err("declare setter cannot have a body".to_owned());
            }
            Some(super::super::stmts::parse_block(s)?)
        } else {
            s.eat_semicolon();
            None
        };
        let full_range = s.span_from(range);
        return Ok(ClassMember::Setter {
            key: s.interner.intern(&key),
            param,
            body,
            modifiers: mods,
            decorators,
            doc,
            range: full_range,
        });
    }

    let key = member_key_name(s)?;
    let type_params = if s.check(TokenKind::LAngle) {
        parse_type_params(s)?
    } else {
        vec![]
    };

    if s.check(TokenKind::LParen) {
        let params = super::super::params::parse_params(s)?;
        let return_type = if s.eat(TokenKind::Colon) {
            Some(parse_type(s)?)
        } else {
            None
        };
        let body = if s.check(TokenKind::LBrace) {
            if class_is_declare {
                return Err("declare method cannot have a body".to_owned());
            }
            Some(super::super::stmts::parse_block(s)?)
        } else {
            s.eat_semicolon();
            None
        };
        let full_range = s.span_from(range);
        return Ok(ClassMember::Method {
            key: s.interner.intern(&key),
            type_params,
            params,
            return_type,
            body,
            modifiers: mods,
            decorators,
            doc,
            range: full_range,
        });
    }

    let type_ann = if s.eat(TokenKind::Colon) {
        Some(parse_type(s)?)
    } else {
        None
    };
    let init = if s.eat(TokenKind::Eq) {
        if class_is_declare {
            return Err("declare property cannot have initializer".to_owned());
        }
        Some(parse_expr(s)?)
    } else {
        None
    };
    s.eat_semicolon();
    let full_range = s.span_from(range);
    Ok(ClassMember::Property {
        key: s.interner.intern(&key),
        type_ann,
        init,
        modifiers: mods,
        decorators,
        doc,
        range: full_range,
    })
}
