use crate::diagnostic::{Diagnostic, DiagnosticKind};
use crate::source::SourceRange;

pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const RED: &str = "\x1b[31m";
pub const BLUE: &str = "\x1b[34m";
pub const YELLOW: &str = "\x1b[33m";
pub const RESET: &str = "\x1b[0m";

pub fn format_diagnostic(diagnostic: &Diagnostic, source: &str) -> String {
    let color = match diagnostic.kind {
        DiagnosticKind::Error => RED,
        DiagnosticKind::Warning => YELLOW,
        DiagnosticKind::Hint => BLUE,
    };

    let kind_str = match diagnostic.kind {
        DiagnosticKind::Error => "error",
        DiagnosticKind::Warning => "warning",
        DiagnosticKind::Hint => "hint",
    };

    let mut max_line = diagnostic.range.start.line;
    for related in &diagnostic.related {
        max_line = max_line.max(related.range.start.line);
    }
    let margin = max_line.to_string().len().max(1);
    let gutter = " ".repeat(margin);

    let mut out = format!(
        "{BOLD}{color}{kind_str}[{code}]{RESET}: {BOLD}{msg}{RESET}\n{gutter} {DIM}┌─{RESET} {path}:{line}:{col}\n{gutter} {DIM}│{RESET}",
        code = diagnostic.code,
        msg = diagnostic.message,
        path = diagnostic.file,
        line = diagnostic.range.start.line,
        col = diagnostic.range.start.column,
    );

    render_snippet(&mut out, source, diagnostic.range, color, margin);

    for related in &diagnostic.related {
        out.push_str(&format!(
            "\n{gutter} {DIM}note{RESET}: {BOLD}{}{RESET}\n{gutter} {DIM}┌─{RESET} {}:{}:{}\n{gutter} {DIM}│{RESET}",
            related.message, related.file, related.range.start.line, related.range.start.column,
        ));
        render_snippet(&mut out, source, related.range, BLUE, margin);
    }

    for suggestion in &diagnostic.suggestions {
        out.push_str(&format!("\n{gutter} {DIM}help{RESET}: {suggestion}"));
    }

    out
}

fn render_snippet(out: &mut String, source: &str, range: SourceRange, color: &str, margin: usize) {
    let line_idx = (range.start.line as usize).saturating_sub(1);
    let src_line = source.lines().nth(line_idx).unwrap_or("");

    let start_col = range.start.column.saturating_sub(1) as usize;
    let end_col = if range.end.line == range.start.line {
        (range.end.column.saturating_sub(1) as usize).max(start_col + 1)
    } else {
        src_line.len().max(start_col + 1)
    };

    let pad = " ".repeat(start_col);
    let underline = "^".repeat(end_col - start_col);
    let gutter = " ".repeat(margin);

    out.push_str(&format!(
        "\n{DIM}{line:>margin$} │{RESET} {src_line}\n{gutter} {DIM}│{RESET} {color}{BOLD}{pad}{underline}{RESET}",
        line = range.start.line,
        margin = margin,
    ));

    out.push_str(&format!("\n{gutter} {DIM}└─{RESET}"));
}
