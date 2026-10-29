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
    let mut depth = 0usize;
    loop {
        if s.peek_line(off) != start_line {
            return false;
        }
        let kind = s.peek_kind(off);
        let opens = matches!(
            kind,
            TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace
        );
        let closes = matches!(
            kind,
            TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace
        );
        if opens {
            depth += 1;
        } else if closes && depth > 0 {
            depth -= 1;
        } else if kind == TokenKind::Colon && depth == 0 {
            return true;
        } else if closes
            || kind == TokenKind::Semicolon
            || kind == TokenKind::EOF
            || kind == TokenKind::Eq
            || (depth == 0 && (kind == TokenKind::Comma || kind == TokenKind::Question))
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
    if matches!(ty.kind, TypeKind::Literal(_)) && s.check(TokenKind::LBracket) {
        return Err("literal types cannot be used as array element types or indexed".to_owned());
    }

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
