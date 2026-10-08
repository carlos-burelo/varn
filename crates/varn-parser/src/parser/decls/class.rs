use super::class_member::parse_class_member;
use crate::expressions::parse_new_callee_expr;
use crate::stream::TokenStream;
use crate::types::{parse_type, parse_type_args, parse_type_params};
use varn_core::ast::operators::Modifiers;
use varn_core::ast::{ClassDecl, Decorator};
use varn_core::TokenKind;

pub fn parse_class_decl(
    s: &mut TokenStream,
    decorators: Vec<Decorator>,
    is_declare: bool,
) -> Result<ClassDecl, String> {
    let range = s.range();
    let is_abstract = s.eat(TokenKind::Abstract);
    s.expect(TokenKind::Class)?;

    let (id, id_offset) = if s.check(TokenKind::Identifier) {
        let id_offset = s.token().range.start.offset;
        let id = s.consume_lexeme();
        (Some(id), id_offset)
    } else {
        (None, 0)
    };
    let type_params = if s.check(TokenKind::LAngle) {
        parse_type_params(s)?
    } else {
        vec![]
    };

    let primary_params = if s.check(TokenKind::LParen) {
        Some(super::super::params::parse_params(s)?)
    } else {
        None
    };

    let super_class = if s.eat(TokenKind::Extends) {
        Some(parse_new_callee_expr(s)?)
    } else {
        None
    };
    let super_type_args = if s.check(TokenKind::LAngle) {
        parse_type_args(s)?
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
    let mut body = vec![];
    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        while s.eat(TokenKind::Semicolon) {}
        while s.check(TokenKind::DocComment) {
            s.advance();
        }
        if s.check(TokenKind::RBrace) {
            break;
        }
        body.push(parse_class_member(s, is_declare)?);
    }
    s.expect(TokenKind::RBrace)?;

    let full_range = s.span_from(range);
    Ok(ClassDecl {
        id,
        ast_id: s.next_ast_id(),
        id_offset,
        type_params,
        primary_params,
        super_class,
        super_type_args,
        implements,
        body,
        modifiers: Modifiers {
            is_abstract,
            is_declare,
            ..Default::default()
        },
        decorators,
        doc: None,
        range: full_range,
    })
}
