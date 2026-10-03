use crate::stream::TokenStream;
use varn_core::ast::TypeNode;
use varn_core::{TokenKind, TypeKind};

pub(crate) fn parse_tuple_type(
    s: &mut TokenStream,
    range: varn_core::SourceRange,
) -> Result<TypeNode, String> {
    s.expect(TokenKind::LBracket)?;
    let mut elements = vec![];
    while !s.check(TokenKind::RBracket) && !s.is_eof() {
        elements.push(super::entry::parse_type(s)?);
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    s.expect(TokenKind::RBracket)?;
    let full_range = s.span_from(range);
    Ok(s.type_node(full_range, TypeKind::Tuple(elements)))
}

pub(crate) fn parse_template_literal_type(s: &mut TokenStream) -> Result<TypeNode, String> {
    let start = s.range();
    let raw = s.consume_lexeme();
    let has_interp = s.interner.resolve(raw).ends_with("${");
    if has_interp {
        loop {
            let _ = super::entry::parse_type(s)?;
            if !matches!(
                s.kind(),
                TokenKind::TemplateMiddle | TokenKind::TemplateTail
            ) {
                return Err(format!(
                    "expected template continuation in type position at {}:{}",
                    s.range().start.line,
                    s.range().start.column
                ));
            }
            let raw_cont = s.consume_lexeme();
            if s.interner.resolve(raw_cont).ends_with('`') {
                break;
            }
        }
    }
    let full_range = s.span_from(start);
    Ok(s.type_node(
        full_range,
        TypeKind::Primitive(varn_core::LangPrimitive::Str),
    ))
}
