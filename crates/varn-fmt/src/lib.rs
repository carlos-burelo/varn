pub fn format_source(source: &str) -> String {
    let mut result = String::with_capacity(source.len() + 64);
    let mut indent_level: usize = 0;
    let mut blank_count: usize = 0;
    let mut in_block_comment = false;
    for raw_line in source.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            if !in_block_comment {
                blank_count += 1;
                if blank_count <= 1 && !result.is_empty() {
                    result.push('\n');
                }
            }
            continue;
        }
        blank_count = 0;
        if in_block_comment {
            let indent = "    ".repeat(indent_level);
            result.push_str(&indent);
            result.push_str(trimmed);
            result.push('\n');
            if trimmed.contains("*/") {
                in_block_comment = false;
            }
            continue;
        }
        if trimmed.starts_with("/*") && !trimmed.contains("*/") {
            in_block_comment = true;
            let indent = "    ".repeat(indent_level);
            result.push_str(&indent);
            result.push_str(trimmed);
            result.push('\n');
            continue;
        }
        let leading_closers = count_leading_closers(trimmed);
        let current_indent = indent_level.saturating_sub(leading_closers);
        let formatted_line = format_line_tokens(trimmed);
        let indent = "    ".repeat(current_indent);
        result.push_str(&indent);
        result.push_str(&formatted_line);
        result.push('\n');
        let (opens, closes) = count_braces_outside_strings(trimmed);
        indent_level = (indent_level + opens).saturating_sub(closes);
    }
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

pub fn apply_indent_style(formatted: &str, insert_spaces: bool, tab_size: u32) -> String {
    if insert_spaces && tab_size == 4 {
        return formatted.to_owned();
    }
    let tab_unit: String = if insert_spaces {
        " ".repeat(tab_size.max(1) as usize)
    } else {
        "\t".to_string()
    };
    let mut out = String::with_capacity(formatted.len());
    for line in formatted.split_inclusive('\n') {
        let stripped = line.trim_start();
        let indent_len = line.len() - stripped.len();
        if indent_len == 0 {
            out.push_str(line);
            continue;
        }
        let prefix = &line[..indent_len];
        let levels = prefix.len() / 4;
        let rest = prefix.len() % 4;
        for _ in 0..levels {
            out.push_str(&tab_unit);
        }
        for _ in 0..rest {
            out.push(' ');
        }
        out.push_str(stripped);
    }
    out
}

fn count_leading_closers(s: &str) -> usize {
    let mut count = 0;
    for c in s.chars() {
        if c == '}' || c == ']' || c == ')' {
            count += 1;
        } else if c.is_whitespace() {
            continue;
        } else {
            break;
        }
    }
    count
}

fn count_braces_outside_strings(s: &str) -> (usize, usize) {
    let mut opens = 0;
    let mut closes = 0;
    let mut in_str = None;
    let mut escaped = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(quote) = in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == quote {
                in_str = None;
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            break;
        }
        match c {
            '"' | '\'' | '`' => in_str = Some(c),
            '{' | '[' => opens += 1,
            '}' | ']' => closes += 1,
            _ => {}
        }
    }
    (opens, closes)
}

fn format_line_tokens(line: &str) -> String {
    if line.starts_with("//") || line.starts_with("/*") || line.starts_with('*') {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len() + 16);
    let mut in_str = None;
    let mut escaped = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(quote) = in_str {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == quote {
                in_str = None;
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
            out.push(c);
            for rem in chars.by_ref() {
                out.push(rem);
            }
            break;
        }
        if c == '"' || c == '\'' || c == '`' {
            in_str = Some(c);
            out.push(c);
            continue;
        }
        if c == ',' {
            out.push(',');
            if chars
                .peek()
                .map(|&next| next != ' ' && next != '\n' && next != ')')
                .unwrap_or(false)
            {
                out.push(' ');
            }
            continue;
        }
        if c == ':' {
            if chars.peek() == Some(&':') {
                chars.next();
                out.push_str("::");
                continue;
            }
            out.push(':');
            if chars
                .peek()
                .map(|&next| next != ' ' && next != ':')
                .unwrap_or(false)
            {
                out.push(' ');
            }
            continue;
        }
        if c == '|' && chars.peek() == Some(&'>') {
            chars.next();
            if !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str("|>");
            if chars.peek() != Some(&' ') {
                out.push(' ');
            }
            continue;
        }
        if c == '=' && chars.peek() == Some(&'>') {
            chars.next();
            if !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str("=>");
            if chars.peek() != Some(&' ') {
                out.push(' ');
            }
            continue;
        }
        out.push(c);
    }
    out
}
