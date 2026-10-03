use crate::stream::TokenStream;
use varn_core::ast::TypeNode;
use varn_core::{TokenKind, TypeKind};

pub(crate) fn parse_union_type(s: &mut TokenStream) -> Result<TypeNode, String> {
    let start = s.range();
    let first = parse_intersection_type(s)?;

    if !s.check(TokenKind::Pipe) {
        return Ok(first);
    }

    let mut members = vec![first];
    while s.eat(TokenKind::Pipe) {
        members.push(parse_intersection_type(s)?);
    }
    let full_range = s.span_from(start);
    Ok(s.type_node(full_range, TypeKind::Union(members)))
}

pub(crate) fn parse_intersection_type(s: &mut TokenStream) -> Result<TypeNode, String> {
    let start = s.range();
    let first = super::suffixes::parse_array_type(s)?;

    if !s.check(TokenKind::Amp) {
        return Ok(first);
    }

    let mut members = vec![first];
    while s.eat(TokenKind::Amp) {
        members.push(super::suffixes::parse_array_type(s)?);
    }
    let full_range = s.span_from(start);
    Ok(s.type_node(full_range, TypeKind::Intersection(members)))
}
