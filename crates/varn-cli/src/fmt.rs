pub use varn_core::diagnostics::format_diagnostic;

pub fn format_error_with_context(
    source: &str,
    path: &str,
    line: u32,
    col: u32,
    kind: &str,
    msg: &str,
) -> String {
    const BOLD: &str = "\x1b[1m";
    const R: &str = "\x1b[0m";
    const DIM: &str = "\x1b[2m";
    const C_CONSTS: &str = "\x1b[38;5;172m";
    const C_ERRORS: &str = "\x1b[31m";

    let src_line = source
        .lines()
        .nth((line as usize).saturating_sub(1))
        .unwrap_or("");
    let col_idx = (col as usize).saturating_sub(1);

    let caret_pad = " ".repeat(col_idx);

    let color = if kind == "warning" {
        C_CONSTS
    } else {
        C_ERRORS
    };

    format!(
        "{BOLD}{color}error[{kind}]{R}: {BOLD}{color}{msg}{R}\n  {DIM}┌─{R} {path}:{line}:{col}\n  {DIM}│{R}\n{DIM} {line} │{R}  {src_line}\n  {DIM}│{R} {color}{BOLD}{caret_pad}^\n  {DIM}└─{R} {color}{BOLD}{msg}{R}",
    )
}
