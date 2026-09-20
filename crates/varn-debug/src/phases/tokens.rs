//! `-p tokens`: lexer token stream (DEBUG_PLAN §4.1).
//!
//! Split into `collect` (data) and `render` (Plain byte-identical / Text
//! diffable) so the byte contract is frozen by a golden and `Text` comes for
//! free.

use std::io::Write;

use varn_core::Token;

use crate::fmt::Format;
use crate::render::{basename, DIM, MAGENTA, RESET, YELLOW};
use crate::report::Report;

/// One row per token: `[index, loc, kind, lexeme]`.
pub fn collect(tokens: &[Token], lexeme_buf: &[u8]) -> Report {
    Report::Rows(
        tokens
            .iter()
            .enumerate()
            .map(|(i, tok)| {
                vec![
                    i.to_string(),
                    format!("{}:{}", tok.range.start.line + 1, tok.range.start.column),
                    format!("{:?}", tok.kind),
                    format!("{:?}", tok.get_lexeme(lexeme_buf)),
                ]
            })
            .collect(),
    )
}

pub fn render(rep: &Report, filename: &str, fmt: Format, w: &mut dyn Write) -> std::io::Result<()> {
    let Report::Rows(rows) = rep else {
        return Ok(());
    };

    match fmt {
        Format::Plain => {
            let pad_len = (50_isize - "tokens".len() as isize - 1).max(0) as usize;
            let padding = "─".repeat(pad_len);
            write!(
                w,
                "\n  {MAGENTA}tokens{RESET} {DIM}{padding} {filename}{RESET}\n"
            )?;
            write!(
                w,
                "  {DIM}{:<5} │ {:<10} │ {:<20} │ Lexeme{RESET}\n",
                "Idx", "Loc", "Kind"
            )?;
            write!(w, "  {}\n", "─".repeat(70))?;
            for r in rows {
                write!(
                    w,
                    "  {DIM}{:<5}{RESET} │ {:<10} │ {MAGENTA}{:<20}{RESET} │ {YELLOW}{}{RESET}\n",
                    r[0], r[1], r[2], r[3]
                )?;
            }
            write!(w, "  {DIM}── end: tokens ──{RESET}\n")?;
        }
        Format::Text => {
            writeln!(w, "# tokens {}", basename(filename))?;
            writeln!(w, "idx|loc|kind|lexeme")?;
            for r in rows {
                writeln!(w, "{}|{}|{}|{}", r[0], r[1], r[2], r[3])?;
            }
        }
    }
    Ok(())
}

pub fn debug_tokens(tokens: &[Token], lexeme_buf: &[u8], filename: &str) {
    let rep = collect(tokens, lexeme_buf);
    let _ = render(&rep, filename, Format::Plain, &mut std::io::stderr());
}
