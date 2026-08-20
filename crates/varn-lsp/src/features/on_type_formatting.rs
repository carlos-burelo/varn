use tower_lsp::lsp_types::{FormattingOptions, Position, Range, TextEdit};

pub fn build_on_type_formatting(
    source: &str,
    position: Position,
    ch: &str,
    options: FormattingOptions,
) -> Option<Vec<TextEdit>> {
    let lines: Vec<&str> = source.lines().collect();
    let current_line_idx = position.line as usize;
    if current_line_idx >= lines.len() {
        return None;
    }

    let line = lines[current_line_idx];
    let tab_size = options.tab_size as usize;
    let insert_spaces = options.insert_spaces;

    if ch == "}" {
        let trimmed = line.trim_start();
        if trimmed.starts_with('}') {
            let target_indent = compute_target_indent(lines.as_slice(), current_line_idx);
            let indent_str = if insert_spaces {
                " ".repeat(target_indent)
            } else {
                "\t".repeat(target_indent / tab_size.max(1))
            };

            let current_leading = line.len() - trimmed.len();
            let edit = TextEdit {
                range: Range {
                    start: Position {
                        line: position.line,
                        character: 0,
                    },
                    end: Position {
                        line: position.line,
                        character: current_leading as u32,
                    },
                },
                new_text: indent_str,
            };
            return Some(vec![edit]);
        }
    }

    None
}

fn compute_target_indent(lines: &[&str], current_line_idx: usize) -> usize {
    let mut depth: i32 = 0;
    for i in (0..current_line_idx).rev() {
        let prev = lines[i].trim();
        for c in prev.chars().rev() {
            if c == '}' {
                depth -= 1;
            } else if c == '{' {
                depth += 1;
                if depth > 0 {
                    let leading = lines[i].len() - lines[i].trim_start().len();
                    return leading;
                }
            }
        }
    }
    0
}
