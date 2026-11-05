use crate::error::{PResult, ParseError};
use crate::stream::TokenStream;
use varn_core::ast::TypeNode;
use varn_core::{TokenKind, TypeKind};

pub(crate) fn parse_template_literal_type(s: &mut TokenStream) -> PResult<TypeNode> {
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
                return Err(ParseError::invalid(format!(
                    "expected template continuation in type position at {}:{}",
                    s.range().start.line,
                    s.range().start.column
                )));
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
