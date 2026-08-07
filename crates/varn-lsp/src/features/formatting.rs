use tower_lsp::lsp_types::{FormattingOptions, Position, Range, TextEdit};

pub fn build_formatting(source: &str, options: FormattingOptions) -> Option<Vec<TextEdit>> {
    let indent_str = if options.insert_spaces {
        " ".repeat(options.tab_size as usize)
    } else {
        "\t".to_string()
    };

    let mut formatted = String::with_capacity(source.len());
    let mut indent_level: usize = 0;

    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return None;
    }

    let total_lines = lines.len();

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            formatted.push('\n');
            continue;
        }

        // Adjust indent for closing braces at start of line
        let starts_closing = trimmed.starts_with('}') || trimmed.starts_with(')');
        if starts_closing && indent_level > 0 {
            indent_level = indent_level.saturating_sub(1);
        }

        for _ in 0..indent_level {
            formatted.push_str(&indent_str);
        }
        formatted.push_str(trimmed);
        formatted.push('\n');

        let open_count = trimmed.chars().filter(|&c| c == '{').count();
        let close_count = trimmed.chars().filter(|&c| c == '}').count();
        if open_count > close_count {
            indent_level += open_count - close_count;
        }
    }

    if formatted == source {
        return None;
    }

    let last_line = (total_lines as u32).saturating_sub(1);
    let last_char = lines.last().map(|l| l.len() as u32).unwrap_or(0);

    Some(vec![TextEdit {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: last_line,
                character: last_char,
            },
        },
        new_text: formatted,
    }])
}
