use tower_lsp_f::lsp_types::{FormattingOptions, Position, Range, TextEdit};

pub fn build_formatting(source: &str, options: FormattingOptions) -> Option<Vec<TextEdit>> {
    let canonical = varn_core::fmt::format_source(source);
    let styled =
        varn_core::fmt::apply_indent_style(&canonical, options.insert_spaces, options.tab_size);
    if styled == source {
        return None;
    }
    let line_count = source.lines().count() as u32;
    let last_len = source.lines().last().map(|l| l.len() as u32).unwrap_or(0);
    Some(vec![TextEdit {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: line_count.max(1) - 1,
                character: last_len,
            },
        },
        new_text: styled,
    }])
}

pub fn build_range_edits(
    source: &str,
    options: FormattingOptions,
    start_line: u32,
    end_line: u32,
) -> Option<Vec<TextEdit>> {
    let canonical = varn_core::fmt::format_source(source);
    let styled =
        varn_core::fmt::apply_indent_style(&canonical, options.insert_spaces, options.tab_size);
    if styled == source {
        return None;
    }
    let src_lines: Vec<&str> = source.lines().collect();
    let fmt_lines: Vec<&str> = styled.lines().collect();
    let mut edits = Vec::new();
    let max = src_lines.len().max(fmt_lines.len()) as u32;
    for idx in start_line..=end_line.min(max.saturating_sub(1)) {
        let old = src_lines.get(idx as usize).copied().unwrap_or("");
        let new = fmt_lines.get(idx as usize).copied().unwrap_or("");
        if old != new {
            edits.push(TextEdit {
                range: Range {
                    start: Position {
                        line: idx,
                        character: 0,
                    },
                    end: Position {
                        line: idx,
                        character: old.len() as u32,
                    },
                },
                new_text: new.to_string(),
            });
        }
    }
    if edits.is_empty() {
        None
    } else {
        Some(edits)
    }
}
