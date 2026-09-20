//! `-p symbols`: symbol table with inferred types (DEBUG_PLAN §4.4).

use std::io::Write;

use varn_checker::CheckResult;

use crate::fmt::Format;
use crate::render::{basename, BLUE, BOLD, DIM, RESET, YELLOW};
use crate::report::Report;

/// Rows `[loc, kind, tag, name, ty]`.
pub fn collect(check_result: &CheckResult) -> Report {
    let mut rows = Vec::new();
    for (id, sym) in check_result.bind.arena.all().iter().enumerate() {
        let loc = format!(
            "{}:{}",
            sym.full_range.start.line + 1,
            sym.full_range.start.column
        );
        let kind = sym.kind.label().trim().to_owned();
        let name = check_result.bind.interner.resolve(sym.name).to_owned();
        let ty = check_result
            .symbol_types
            .get(&id)
            .or(sym.ty.as_ref())
            .map(|t| {
                t.display(&check_result.bind.ty_table, &check_result.bind.interner)
                    .to_string()
            })
            .unwrap_or_else(|| "dynamic".to_string());

        let origin = sym
            .origin_module
            .map(|a| check_result.bind.interner.resolve(a))
            .unwrap_or("");

        let is_core = origin.starts_with("core:")
            || origin.starts_with("builtin:")
            || (origin.is_empty() && sym.full_range.start.line == 0);
        let is_std = !is_core && origin.starts_with("std:");
        let tag = if is_core {
            "[core]"
        } else if is_std {
            "[std]"
        } else {
            "[usr]"
        };

        rows.push(vec![loc, kind, tag.to_owned(), name, ty]);
    }
    Report::Rows(rows)
}

pub fn render(rep: &Report, filename: &str, fmt: Format, w: &mut dyn Write) -> std::io::Result<()> {
    let Report::Rows(rows) = rep else {
        return Ok(());
    };

    match fmt {
        Format::Plain => {
            let pad_len = (50_isize - "type inference engine".len() as isize - 1).max(0) as usize;
            let padding = "─".repeat(pad_len);
            write!(
                w,
                "\n  {BLUE}type inference engine{RESET} {DIM}{padding} {filename}{RESET}\n"
            )?;
            write!(w, "  Symbol Types\n")?;
            write!(
                w,
                "  {DIM}{:<8} │ {:<15} │ {:<20} │ Type Details{RESET}\n",
                "Loc", "Kind", "Name"
            )?;
            write!(w, "  {}\n", "─".repeat(80))?;
            for r in rows {
                write!(
                    w,
                    "  {DIM}{:<8}{RESET} │ {:<15} │ {DIM}{}{RESET} {BOLD}{:<20}{RESET} │ {YELLOW}{}{RESET}\n",
                    r[0], r[1], r[2], r[3], r[4]
                )?;
            }
            write!(w, "\n")?;
            write!(w, "  {DIM}── end: type inference engine ──{RESET}\n")?;
        }
        Format::Text => {
            writeln!(w, "# symbols {}", basename(filename))?;
            writeln!(w, "loc|kind|tag|name|ty")?;
            for r in rows {
                writeln!(w, "{}", r.join("|"))?;
            }
        }
    }
    Ok(())
}

pub fn debug_symbols(
    check_result: &CheckResult,
    filename: &str,
    _flags: &crate::flags::DebugFlags,
) {
    let rep = collect(check_result);
    let _ = render(&rep, filename, Format::Plain, &mut std::io::stderr());
}
