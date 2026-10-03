use crate::stream::TokenStream;
use varn_core::ast::TypeNode;
use varn_core::{TokenKind, TypeKind};

pub(crate) fn is_ternary_at(s: &TokenStream) -> bool {
    if s.kind() != TokenKind::Question {
        return false;
    }
    let start_line = s.peek_line(0);
    let mut off = 1;
    let next = s.peek_kind(off);
    if next == TokenKind::Colon {
        return false;
    }
    loop {
        if s.peek_line(off) != start_line {
            return false;
        }
        let kind = s.peek_kind(off);
        if kind == TokenKind::Colon {
            return true;
        }
        if kind == TokenKind::Semicolon
            || kind == TokenKind::Comma
            || kind == TokenKind::RParen
            || kind == TokenKind::RBracket
            || kind == TokenKind::RBrace
            || kind == TokenKind::EOF
            || kind == TokenKind::Question
            || kind == TokenKind::Eq
            || kind.starts_statement()
        {
            return false;
        }
        off += 1;
    }
}

pub(crate) fn parse_array_type(s: &mut TokenStream) -> Result<TypeNode, String> {
    let start = s.range();
    let mut ty = super::primary::parse_primary_type(s)?;

    loop {
        if s.check(TokenKind::LBracket) && s.peek_kind(1) == TokenKind::RBracket {
            s.advance();
            s.advance();
            let full_range = s.span_from(start);
            ty = s.type_node(full_range, TypeKind::Array(Box::new(ty)));
        } else if s.check(TokenKind::LBracket) && s.peek_kind(1) != TokenKind::RBracket {
            s.advance();
            let index = super::entry::parse_type(s)?;
            s.expect(TokenKind::RBracket)?;
            let full_range = s.span_from(start);
            ty = s.type_node(
                full_range,
                TypeKind::IndexedAccess {
                    object: Box::new(ty),
                    index: Box::new(index),
                },
            );
        } else if s.check(TokenKind::Question) && !is_ternary_at(s) {
            let q_range = s.range();
            s.advance();
            let full_range = s.span_from(start);
            let null_node =
                s.type_node(q_range, TypeKind::Primitive(varn_core::LangPrimitive::Null));
            ty = s.type_node(full_range, TypeKind::Union(vec![ty, null_node]));
        } else {
            break;
        }
    }
    Ok(ty)
}
