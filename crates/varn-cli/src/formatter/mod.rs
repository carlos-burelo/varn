use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use crate::cli::FmtArgs;
use crate::error::CliError;


pub fn run_fmt(args: FmtArgs) -> Result<(), CliError> {
    let start_time = Instant::now();
    let target = args.path.as_deref().unwrap_or(".");
    let files = discover_vn_files(Path::new(target))?;

    if files.is_empty() {
        if args.verbose {
            println!("No .vn files found to format in '{target}'");
        }
        return Ok(());
    }

    let mut changed = 0;
    let mut unformatted = Vec::new();

    for file in &files {
        let content = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(e) => {
                return Err(CliError::fatal(format!(
                    "Failed to read {}: {e}",
                    file.display()
                )));
            }
        };

        let formatted = format_source(&content);

        if content != formatted {
            changed += 1;
            if args.check {
                unformatted.push(file.clone());
            } else {
                if let Err(e) = fs::write(file, &formatted) {
                    return Err(CliError::fatal(format!(
                        "Failed to write {}: {e}",
                        file.display()
                    )));
                }
                if args.verbose {
                    println!("  \x1b[32mformatted\x1b[0m {}", file.display());
                }
            }
        }
    }

    let elapsed = start_time.elapsed();

    if args.check {
        if !unformatted.is_empty() {
            eprintln!(
                "\n\x1b[31merror\x1b[0m: {} file{} not properly formatted:\n",
                unformatted.len(),
                if unformatted.len() == 1 { "" } else { "s" }
            );
            for f in &unformatted {
                eprintln!("  - {}", f.display());
            }
            eprintln!("\nRun \x1b[1mvn fmt\x1b[0m to format these files.\n");
            return Err(CliError::usage(format!(
                "{} unformatted files found",
                unformatted.len()
            )));
        } else {
            println!(
                "\n  \x1b[1;32m✓\x1b[0m All {} .vn file{} correctly formatted ({:.2?})\n",
                files.len(),
                if files.len() == 1 { "" } else { "s" },
                elapsed
            );
        }
    } else {
        println!(
            "\n  \x1b[1;36mvarn fmt\x1b[0m · {} file{} checked, {} formatted ({:.2?})\n",
            files.len(),
            if files.len() == 1 { "" } else { "s" },
            changed,
            elapsed
        );
    }

    Ok(())
}

fn discover_vn_files(path: &Path) -> Result<Vec<PathBuf>, CliError> {
    let mut files = Vec::new();
    if path.is_file() {
        if path.extension().and_then(|e| e.to_str()) == Some("vn") {
            files.push(path.to_path_buf());
        }
        return Ok(files);
    }

    if path.is_dir() {
        collect_vn_recursive(path, &mut files)?;
        files.sort();
    }

    Ok(files)
}

fn collect_vn_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), CliError> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            collect_vn_recursive(&p, out)?;
        } else if p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("vn") {
            out.push(p);
        }
    }
    Ok(())
}

/// Formats Varn source code with standard 4-space indentation, normalized operators and clean lines.
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
                // Allow at most 1 blank line consecutively
                if blank_count <= 1 && !result.is_empty() {
                    result.push('\n');
                }
            }
            continue;
        }

        blank_count = 0;

        // Handle block comments /* ... */
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

        // Adjust indent for closing braces/brackets on this line
        let leading_closers = count_leading_closers(trimmed);
        let current_indent = indent_level.saturating_sub(leading_closers);

        let formatted_line = format_line_tokens(trimmed);

        let indent = "    ".repeat(current_indent);
        result.push_str(&indent);
        result.push_str(&formatted_line);
        result.push('\n');

        // Calculate delta for next line
        let (opens, closes) = count_braces_outside_strings(trimmed);
        indent_level = (indent_level + opens).saturating_sub(closes);
    }

    // Ensure trailing newline
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }

    result
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

        // Check for line comment //
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
    // Preserve comments verbatim
    if line.starts_with("//") || line.starts_with("/*") || line.starts_with('*') {
        return line.to_string();
    }

    let mut out = String::with_capacity(line.len() + 16);
    let mut in_str = None;
    let mut escaped = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        // String literal pass-through
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

        // Line comment pass-through
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

        // Spacing around commas: `a, b`
        if c == ',' {
            out.push(',');
            if chars.peek().map(|&next| next != ' ' && next != '\n' && next != ')').unwrap_or(false) {
                out.push(' ');
            }
            continue;
        }

        // Spacing after colons: `foo: int`, but not `::`
        if c == ':' {
            if chars.peek() == Some(&':') {
                chars.next();
                out.push_str("::");
                continue;
            }
            out.push(':');
            if chars.peek().map(|&next| next != ' ' && next != ':').unwrap_or(false) {
                out.push(' ');
            }
            continue;
        }

        // Spacing around binary operators like `===`, `!==`, `==`, `!=`, `=>`, `|>`, `??`
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
