use super::diagnostic::{Diagnostic, DiagnosticKind};
use crate::source::SourceRange;
use crate::term::chalk::{chalk, Chalk};

const TAB_WIDTH: usize = 4;

pub fn display_path(path: &str) -> String {
    path.strip_prefix(r"\\?\").unwrap_or(path).replace('\\', "/")
}

struct Place<'a> {
    line: usize,
    column: usize,
    text: &'a str,
}

fn place(source: &str, offset: u32) -> Place<'_> {
    let at = (offset as usize).min(source.len());
    let at = (0..=at)
        .rev()
        .find(|i| source.is_char_boundary(*i))
        .unwrap_or(0);
    let line_start = source[..at].rfind('\n').map_or(0, |i| i + 1);
    let line_end = source[at..].find('\n').map_or(source.len(), |i| at + i);
    Place {
        line: source[..line_start].matches('\n').count() + 1,
        column: source[line_start..at].chars().count() + 1,
        text: source[line_start..line_end].trim_end_matches('\r'),
    }
}

fn expand_tabs(text: &str) -> String {
    text.replace('\t', &" ".repeat(TAB_WIDTH))
}

fn width_of(text: &str) -> usize {
    console::measure_text_width(&expand_tabs(text))
}

fn severity_label(kind: DiagnosticKind) -> Chalk {
    match kind {
        DiagnosticKind::Error => chalk("error").red().bold(),
        DiagnosticKind::Warning => chalk("warning").yellow().bold(),
        DiagnosticKind::Hint => chalk("hint").blue().bold(),
    }
}

fn accent(kind: DiagnosticKind, text: &str) -> Chalk {
    match kind {
        DiagnosticKind::Error => chalk(text).red().bold(),
        DiagnosticKind::Warning => chalk(text).yellow().bold(),
        DiagnosticKind::Hint => chalk(text).blue().bold(),
    }
}

pub fn format_diagnostic(diagnostic: &Diagnostic, source: &str) -> String {
    let start = place(source, diagnostic.range.start.offset);
    let max_line = diagnostic
        .related
        .iter()
        .map(|related| related.range.start.line as usize)
        .chain([start.line])
        .max()
        .unwrap_or(1);
    let margin = max_line.to_string().len();
    let gutter = " ".repeat(margin);
    let bar = chalk("│").dim();
    let corner = chalk("┌─").dim();

    let mut out = format!(
        "{}{}{}{}: {}\n{gutter} {corner} {}:{}:{}\n{gutter} {bar}",
        severity_label(diagnostic.kind),
        chalk("[").bold(),
        accent(diagnostic.kind, &diagnostic.code.to_string()),
        chalk("]").bold(),
        chalk(&diagnostic.message).bold(),
        display_path(&diagnostic.file),
        start.line,
        start.column,
    );
    render_snippet(&mut out, source, diagnostic.range, diagnostic.kind, margin);

    for related in &diagnostic.related {
        let related_start = place(source, related.range.start.offset);
        out.push_str(&format!(
            "\n{gutter} {}: {}\n{gutter} {corner} {}:{}:{}\n{gutter} {bar}",
            chalk("note").dim(),
            chalk(&related.message).bold(),
            display_path(&related.file),
            related_start.line,
            related_start.column,
        ));
        if *related.file == *diagnostic.file {
            render_snippet(&mut out, source, related.range, DiagnosticKind::Hint, margin);
        }
    }

    for suggestion in &diagnostic.suggestions {
        out.push_str(&format!(
            "\n{gutter} {}: {suggestion}",
            chalk("help").dim()
        ));
    }

    out
}

fn render_snippet(
    out: &mut String,
    source: &str,
    range: SourceRange,
    kind: DiagnosticKind,
    margin: usize,
) {
    let first = place(source, range.start.offset);
    let last = place(source, range.end.offset.max(range.start.offset));
    let gutter = " ".repeat(margin);
    let bar = chalk("│").dim();

    let emit = |out: &mut String, line: usize, text: &str, from: usize, to: usize| {
        let shown = expand_tabs(text);
        let pad = " ".repeat(from);
        let underline = "^".repeat(to.saturating_sub(from).max(1));
        out.push_str(&format!(
            "\n{} {bar} {shown}\n{gutter} {bar} {pad}{}",
            chalk(format!("{line:>margin$}")).dim(),
            accent(kind, &underline),
        ));
    };

    let first_prefix: String = first.text.chars().take(first.column - 1).collect();
    let from = width_of(&first_prefix);
    if last.line == first.line {
        let covered: String = first
            .text
            .chars()
            .skip(first.column - 1)
            .take(last.column.saturating_sub(first.column))
            .collect();
        emit(out, first.line, first.text, from, from + width_of(&covered));
    } else {
        emit(out, first.line, first.text, from, width_of(first.text));
        if last.line > first.line + 1 {
            out.push_str(&format!("\n{gutter} {}", chalk("·").dim()));
        }
        let indent = first_non_blank(last.text);
        let last_prefix: String = last.text.chars().take(last.column - 1).collect();
        emit(
            out,
            last.line,
            last.text,
            width_of(&last.text[..indent]),
            width_of(&last_prefix),
        );
    }

    out.push_str(&format!("\n{gutter} {}", chalk("└─").dim()));
}

fn first_non_blank(text: &str) -> usize {
    text.len() - text.trim_start().len()
}

pub fn render_report(file: &str, source: &str, diagnostics: &super::DiagnosticBag) -> String {
    let rendered: Vec<String> = diagnostics
        .iter()
        .map(|d| format_diagnostic(d, source))
        .collect();
    let errors = diagnostics.iter().filter(|d| d.is_error()).count();
    let warnings = diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::Warning)
        .count();
    format!(
        "{}\n\n{}: could not compile `{}` due to {}{}",
        rendered.join("\n"),
        chalk("error").red().bold(),
        display_path(file),
        count_phrase(errors, "error"),
        if warnings > 0 {
            format!("; {}", count_phrase(warnings, "warning"))
        } else {
            String::new()
        },
    )
}

fn count_phrase(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}
