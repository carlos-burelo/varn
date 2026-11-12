use crate::error::{PResult, ParseError};
use crate::stream::TokenStream;
use crate::types::parse_type;
use vn_core::ast::decl::ExtensionMember;
use vn_core::ast::operators::Modifiers;
use vn_core::ast::ExtensionDecl;
use vn_core::ast::FunctionDecl;
use vn_core::TokenKind;

pub fn parse_extension_decl(s: &mut TokenStream) -> PResult<ExtensionDecl> {
    let range = s.range();
    s.expect(TokenKind::Extension)?;

    if s.check(TokenKind::For) {
        return Err(ParseError::invalid(
            "extension syntax changed; use `extension [Name] on Type { method() { ... } }`",
        ));
    }

    let id = if s.check(TokenKind::On) || s.check(TokenKind::LAngle) {
        None
    } else {
        Some(s.expect_id()?)
    };

    let type_params = if s.check(TokenKind::LAngle) {
        crate::types::parse_type_params(s)?
    } else {
        Vec::new()
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
            return Err(ParseError::invalid(
                "extension members use ECMAScript-style method syntax; remove the `function` keyword",
            ));
        }
        members.push(parse_extension_member(s)?);
    }
    s.expect(TokenKind::RBrace)?;

    let full_range = s.span_from(range);
    Ok(ExtensionDecl {
        id,
        ast_id: s.next_ast_id(),
        type_params,
        target,
        members,
        range: full_range,
    })
}

fn parse_extension_member(s: &mut TokenStream) -> PResult<ExtensionMember> {
    let range = s.range();
    let is_declare = s.eat(TokenKind::Declare);
    let is_async = s.eat(TokenKind::Async);
    let is_generator = s.eat(TokenKind::Star);

    let is_get = s.check(TokenKind::Get) && {
        let nk = s.peek_kind(1);
        nk != TokenKind::LParen
            && nk != TokenKind::LAngle
            && nk != TokenKind::Semicolon
            && nk != TokenKind::Colon
    };
    let is_set = s.check(TokenKind::Set) && {
        let nk = s.peek_kind(1);
        nk != TokenKind::LParen
            && nk != TokenKind::LAngle
            && nk != TokenKind::Semicolon
            && nk != TokenKind::Colon
    };

    if is_declare && (is_get || is_set) {
        s.advance();
        s.expect_id()?;
        super::super::params::parse_params(s)?;
        if s.eat(TokenKind::Colon) {
            parse_type(s)?;
        }
        s.eat_semicolon();
        return Err(ParseError::invalid(
            "`declare` applies to extension methods: a getter or setter has a body".to_owned(),
        ));
    }

    if is_get {
        if is_async || is_generator {
            return Err(ParseError::invalid(
                "extension getters cannot be async or generators".to_owned(),
            ));
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
            return Err(ParseError::invalid(
                "extension setters cannot be async or generators".to_owned(),
            ));
        }
        s.advance();
        let key = s.expect_id()?;
        s.expect(TokenKind::LParen)?;
        let param = super::super::params::parse_single_param(s)?;
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
    let params = super::super::params::parse_params(s)?;
    let return_type = if s.eat(TokenKind::Colon) {
        Some(parse_type(s)?)
    } else {
        None
    };
    let body = if is_declare {
        if s.check(TokenKind::LBrace) {
            return Err(ParseError::invalid(
                "a `declare` extension method forwards to the receiver and has no body".to_owned(),
            ));
        }
        s.eat_semicolon();
        s.stmt(s.range(), vn_core::ast::StmtKind::Empty)
    } else {
        super::super::stmts::parse_block(s)?
    };

    let full_range = s.span_from(range);
    Ok(ExtensionMember::Method(FunctionDecl {
        id,
        ast_id: s.next_ast_id(),
        id_offset,
        type_params,
        params,
        return_type,
        body,
        modifiers: Modifiers {
            is_async,
            is_generator,
            is_declare,
            ..Default::default()
        },
        decorators: vec![],
        doc: None,
        range: full_range,
    }))
}
