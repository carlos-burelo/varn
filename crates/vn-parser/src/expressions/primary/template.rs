use crate::error::PResult;
use crate::expressions::literal_text::unescape_reporting;
use crate::stream::TokenStream;
use vn_core::ast::{ExprId, TemplatePart};

pub(crate) fn parse_template(s: &mut TokenStream) -> PResult<ExprId> {
    let range = s.range();
    let mut parts = vec![];

    let raw = s.consume_lexeme();
    let raw_text = s.interner.resolve(raw).to_owned();
    let literal_text = raw_text.trim_start_matches('`');
    let (literal_text, is_head) = if let Some(text) = literal_text.strip_suffix("${") {
        (text, true)
    } else {
        (literal_text.trim_end_matches('`'), false)
    };
    let head_text = literal_text.to_owned();
    let head_value = unescape_reporting(s, &head_text, range, range.start.offset + 1);
    parts.push(TemplatePart::Literal(head_value));

    if !is_head {
        return Ok(s.expr(range, vn_core::ast::ExprKind::Template { parts }));
    }

    loop {
        let interp = super::super::parse_expr(s)?;
        parts.push(TemplatePart::Interpolation(interp));

        let raw_cont = s.consume_lexeme();
        let raw_cont_text = s.interner.resolve(raw_cont).to_owned();
        let (content, is_tail) = if let Some(text) = raw_cont_text.strip_suffix('`') {
            (text.strip_prefix('}').unwrap_or(text), true)
        } else {
            let after_close = raw_cont_text.strip_prefix('}').unwrap_or(&raw_cont_text);
            (after_close.trim_end_matches("${"), false)
        };
        let content_text = content.to_owned();
        let cont_range = s.range();
        let content_value =
            unescape_reporting(s, &content_text, cont_range, cont_range.start.offset);
        parts.push(TemplatePart::Literal(content_value));

        if is_tail {
            break;
        }
    }

    Ok(s.expr(range, vn_core::ast::ExprKind::Template { parts }))
}
