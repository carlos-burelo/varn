use crate::stream::TokenStream;
use varn_core::ast::TypeNode;
use varn_core::{TokenKind, TypeKind};

pub fn parse_type(s: &mut TokenStream) -> Result<TypeNode, String> {
    let start = s.range();

    if s.check(TokenKind::Identifier) && s.peek_kind(1) == TokenKind::Is {
        let param_name = s.consume_lexeme();
        s.advance();
        let target_ty = parse_type(s)?;
        let full_range = s.span_from(start);
        return Ok(s.type_node(
            full_range,
            TypeKind::TypePredicate {
                parameter_name: param_name,
                target_type: Box::new(target_ty),
            },
        ));
    }

    let ty = super::combinators::parse_union_type(s)?;

    if s.eat(TokenKind::Extends) {
        let extends_ty = super::combinators::parse_union_type(s)?;
        s.expect(TokenKind::Question)?;
        let true_ty = parse_type(s)?;
        s.expect(TokenKind::Colon)?;
        let false_ty = parse_type(s)?;
        let full_range = s.span_from(start);
        return Ok(s.type_node(
            full_range,
            TypeKind::Conditional {
                check: Box::new(ty),
                extends: Box::new(extends_ty),
                true_type: Box::new(true_ty),
                false_type: Box::new(false_ty),
            },
        ));
    }

    Ok(ty)
}
