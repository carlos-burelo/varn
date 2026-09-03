use tower_lsp::lsp_types::{FormattingOptions, Position, Range, TextEdit};

pub fn build_formatting(source: &str, options: FormattingOptions) -> Option<Vec<TextEdit>> {
    let tab_unit = if options.insert_spaces {
        " ".repeat(options.tab_size as usize)
    } else {
        "\t".to_string()
    };

    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return None;
    }

    let mut edits = Vec::new();
    let mut current_indent: usize = 0;
    let mut in_multiline_comment = false;

    for (line_idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // If the empty line has trailing whitespace, clear it
            if !line.is_empty() {
                edits.push(TextEdit {
                    range: Range {
                        start: Position {
                            line: line_idx as u32,
                            character: 0,
                        },
                        end: Position {
                            line: line_idx as u32,
                            character: line.len() as u32,
                        },
                    },
                    new_text: String::new(),
                });
            }
            continue;
        }

        if in_multiline_comment {
            if trimmed.contains("*/") {
                in_multiline_comment = false;
            }
            continue;
        }

        if trimmed.starts_with("/*") && !trimmed.contains("*/") {
            in_multiline_comment = true;
            continue;
        }

        // Count leading closing tokens on this line to dedent before printing
        let starts_closing =
            trimmed.starts_with('}') || trimmed.starts_with(']') || trimmed.starts_with(')');

        let line_effective_indent = if starts_closing && current_indent > 0 {
            current_indent.saturating_sub(1)
        } else {
            current_indent
        };

        let target_prefix = tab_unit.repeat(line_effective_indent);
        let current_leading_len = line.len() - line.trim_start().len();
        let current_prefix = &line[..current_leading_len];

        if current_prefix != target_prefix {
            edits.push(TextEdit {
                range: Range {
                    start: Position {
                        line: line_idx as u32,
                        character: 0,
                    },
                    end: Position {
                        line: line_idx as u32,
                        character: current_leading_len as u32,
                    },
                },
                new_text: target_prefix,
            });
        }

        // Update indent level for following lines by scanning tokens ignoring strings and line comments
        let delta = compute_line_indent_delta(trimmed);
        if delta > 0 {
            current_indent += delta as usize;
        } else if delta < 0 {
            current_indent = current_indent.saturating_sub((-delta) as usize);
        }
    }

    if edits.is_empty() {
        None
    } else {
        Some(edits)
    }
}

fn compute_line_indent_delta(line: &str) -> i32 {
    let bytes = line.as_bytes();
    let mut delta = 0i32;
    let mut in_string = false;
    let mut quote_char = b'"';
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];

        if in_string {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == quote_char {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if b == b'"' || b == b'\'' || b == b'`' {
            in_string = true;
            quote_char = b;
            i += 1;
            continue;
        }

        // Ignore line comments
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            break;
        }

        match b {
            b'{' | b'[' | b'(' => delta += 1,
            b'}' | b']' | b')' => delta -= 1,
            _ => {}
        }
        i += 1;
    }

    delta
}
