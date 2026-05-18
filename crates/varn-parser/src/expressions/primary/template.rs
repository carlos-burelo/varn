use crate::expressions::helpers::unescape_string;
use crate::stream::TokenStream;
use varn_core::ast::{Expr, TemplatePart};

pub(super) fn parse_template(s: &mut TokenStream) -> Result<Expr, String> {
    let range = s.range();
    let mut parts = vec![];

    let raw = s.consume_lexeme();
    let literal_text = raw.trim_start_matches('`');
    let (literal_text, is_head) = if let Some(text) = literal_text.strip_suffix("${") {
        (text, true)
    } else {
        (literal_text.trim_end_matches('`'), false)
    };
    parts.push(TemplatePart::Literal(unescape_string(literal_text)));

    if !is_head {
        return Ok(Expr::new_with_range(
            range,
            varn_core::ast::ExprKind::Template { parts },
        ));
    }

    loop {
        let interp = super::super::parse_seq_expr(s)?;
        parts.push(TemplatePart::Interpolation(interp));

        let raw_cont = s.consume_lexeme();
        let (content, is_tail) = if let Some(text) = raw_cont.strip_suffix('`') {
            (text.strip_prefix('}').unwrap_or(text), true)
        } else {
            let after_close = raw_cont.strip_prefix('}').unwrap_or(&raw_cont);
            (after_close.trim_end_matches("${"), false)
        };
        parts.push(TemplatePart::Literal(unescape_string(content)));

        if is_tail {
            break;
        }
    }

    Ok(Expr::new_with_range(
        range,
        varn_core::ast::ExprKind::Template { parts },
    ))
}
