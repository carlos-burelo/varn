use crate::expressions::helpers::unescape_string;
use crate::stream::TokenStream;
use varn_core::ast::{ExprId, TemplatePart};

pub(crate) fn parse_template(s: &mut TokenStream) -> Result<ExprId, String> {
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
    parts.push(TemplatePart::Literal(unescape_string(literal_text)));

    if !is_head {
        return Ok(s.expr(range, varn_core::ast::ExprKind::Template { parts }));
    }

    loop {
        let interp = super::super::parse_seq_expr(s)?;
        parts.push(TemplatePart::Interpolation(interp));

        let raw_cont = s.consume_lexeme();
        let raw_cont_text = s.interner.resolve(raw_cont).to_owned();
        let (content, is_tail) = if let Some(text) = raw_cont_text.strip_suffix('`') {
            (text.strip_prefix('}').unwrap_or(text), true)
        } else {
            let after_close = raw_cont_text.strip_prefix('}').unwrap_or(&raw_cont_text);
            (after_close.trim_end_matches("${"), false)
        };
        parts.push(TemplatePart::Literal(unescape_string(content)));

        if is_tail {
            break;
        }
    }

    Ok(s.expr(range, varn_core::ast::ExprKind::Template { parts }))
}
