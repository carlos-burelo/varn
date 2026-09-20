//! `-p scope`: static scope tree (DEBUG_PLAN §4.8). Only the root proto and its
//! constant-pool strings, no recursion to nested functions (preserved today).

use std::io::Write;

use varn_compiler::FunctionProto;
use varn_types::chunk::{Literal, PoolEntry};

use crate::fmt::Format;
use crate::render::{basename, BLUE, BOLD, DIM, MAGENTA, RESET};
use crate::report::Report;

/// Rows: `["fn", depth, name, upvalues]` or `["consts", depth, joined-ids]`.
pub fn collect(proto: &FunctionProto) -> Report {
    let mut rows = Vec::new();
    let name = proto.name.as_deref().unwrap_or("<anonymous>");
    rows.push(vec![
        "fn".to_owned(),
        "0".to_owned(),
        name.to_owned(),
        proto.upvalue_count.to_string(),
    ]);

    let ids: Vec<&str> = proto
        .chunk
        .constants
        .iter()
        .filter_map(|c| {
            if let PoolEntry::Literal(Literal::Str(s)) = c {
                Some(s.as_ref())
            } else {
                None
            }
        })
        .collect();
    if !ids.is_empty() {
        rows.push(vec!["consts".to_owned(), "0".to_owned(), ids.join(", ")]);
    }
    Report::Rows(rows)
}

pub fn render(rep: &Report, filename: &str, fmt: Format, w: &mut dyn Write) -> std::io::Result<()> {
    let Report::Rows(rows) = rep else {
        return Ok(());
    };

    match fmt {
        Format::Plain => {
            let pad_len = (50_isize - "static scope tree".len() as isize - 1).max(0) as usize;
            let padding = "─".repeat(pad_len);
            write!(
                w,
                "\n  {MAGENTA}static scope tree{RESET} {DIM}{padding} {filename}{RESET}\n"
            )?;
            for r in rows {
                let depth: usize = r[1].parse().unwrap_or(0);
                let indent = "    ".repeat(depth);
                if r[0] == "fn" {
                    let marker = if depth == 0 {
                        "└── "
                    } else {
                        "    └── "
                    };
                    let outer = if depth == 0 {
                        ""
                    } else {
                        &indent[..indent.len() - 4]
                    };
                    write!(
                        w,
                        "{outer}{marker}{BOLD}fn{RESET} {BLUE}{}{RESET} (upvalues: {})\n",
                        r[2], r[3]
                    )?;
                } else {
                    write!(w, "{indent}    {DIM}const pool strings: {}{RESET}\n", r[2])?;
                }
            }
            write!(w, "  {DIM}── end: static scope tree ──{RESET}\n")?;
        }
        Format::Text => {
            writeln!(w, "# scope {}", basename(filename))?;
            writeln!(w, "kind|depth|a|b")?;
            for r in rows {
                writeln!(w, "{}", r.join("|"))?;
            }
        }
    }
    Ok(())
}

pub fn debug_scopes(proto: &FunctionProto, filename: &str) {
    let rep = collect(proto);
    let _ = render(&rep, filename, Format::Plain, &mut std::io::stderr());
}
