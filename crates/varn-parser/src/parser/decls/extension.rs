use crate::stream::TokenStream;
use crate::types::parse_type;
use varn_core::ast::decl::ExtensionMember;
use varn_core::ast::operators::Modifiers;
use varn_core::ast::ExtensionDecl;
use varn_core::ast::FunctionDecl;
use varn_core::TokenKind;

pub fn parse_extension_decl(s: &mut TokenStream) -> Result<ExtensionDecl, String> {
    let range = s.range();
    s.expect(TokenKind::Extension)?;

    if s.check(TokenKind::For) {
        return Err(
            "extension syntax changed; use `extension [Name] on Type { method() { ... } }`"
                .to_owned(),
        );
    }

    let id = if s.check(TokenKind::On) {
        None
    } else {
        Some(s.expect_id()?)
    };

    s.expect(TokenKind::On)?;

    let target = parse_type(s)?;

    s.expect(TokenKind::LBrace)?;
    let mut members: Vec<ExtensionMember> = vec![];
    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        while s.eat(TokenKind::Semicolon) {}
        if s.check(TokenKind::RBrace) {
            break;
        }
        if s.check(TokenKind::Function)
            || (s.check(TokenKind::Async) && s.peek_kind(1) == TokenKind::Function)
        {
            return Err(
                "extension members use ECMAScript-style method syntax; remove the `function` keyword"
                    .to_owned(),
            );
        }
        members.push(parse_extension_member(s)?);
    }
    s.expect(TokenKind::RBrace)?;

    let full_range = s.span_from(range);
    Ok(ExtensionDecl {
        id,
        ast_id: 0,
        target,
        members,
        range: full_range,
    })
}

fn parse_extension_member(s: &mut TokenStream) -> Result<ExtensionMember, String> {
    let range = s.range();
    let is_async = s.eat(TokenKind::Async);
    let is_generator = s.eat(TokenKind::Star);

    let is_get = s.check(TokenKind::Get) && {
        let nk = s.peek_kind(1);
        nk != TokenKind::LParen && nk != TokenKind::Semicolon && nk != TokenKind::Colon
    };
    let is_set = s.check(TokenKind::Set) && {
        let nk = s.peek_kind(1);
        nk != TokenKind::LParen && nk != TokenKind::Semicolon && nk != TokenKind::Colon
    };

    if is_get {
        if is_async || is_generator {
            return Err("extension getters cannot be async or generators".to_owned());
        }
        s.advance();
        let key = s.expect_id()?;
        s.expect(TokenKind::LParen)?;
        s.expect(TokenKind::RParen)?;
        let return_type = if s.eat(TokenKind::Colon) {
            Some(parse_type(s)?)
        } else {
            None
        };
        let body = super::super::stmts::parse_block(s)?;
        let full_range = s.span_from(range);
        return Ok(ExtensionMember::Getter {
            key,
            return_type,
            body,
            modifiers: Modifiers::default(),
            range: full_range,
        });
    }

    if is_set {
        if is_async || is_generator {
            return Err("extension setters cannot be async or generators".to_owned());
        }
        s.advance();
        let key = s.expect_id()?;
        s.expect(TokenKind::LParen)?;
        let param = super::super::patterns::parse_single_param(s)?;
        s.expect(TokenKind::RParen)?;
        let body = super::super::stmts::parse_block(s)?;
        let full_range = s.span_from(range);
        return Ok(ExtensionMember::Setter {
            key,
            param,
            body,
            modifiers: Modifiers::default(),
            range: full_range,
        });
    }

    let id_offset = s.range().start.offset;
    let id = s.expect_id()?;
    let type_params = if s.check(TokenKind::LAngle) {
        crate::types::parse_type_params(s)?
    } else {
        vec![]
    };
    let params = super::super::patterns::parse_params(s)?;
    let return_type = if s.eat(TokenKind::Colon) {
        Some(parse_type(s)?)
    } else {
        None
    };
    let body = super::super::stmts::parse_block(s)?;

    let full_range = s.span_from(range);
    Ok(ExtensionMember::Method(FunctionDecl {
        id,
        ast_id: 0,
        id_offset,
        type_params,
        params,
        return_type,
        body,
        modifiers: Modifiers {
            is_async,
            is_generator,
            ..Default::default()
        },
        decorators: vec![],
        doc: None,
        range: full_range,
    }))
}
