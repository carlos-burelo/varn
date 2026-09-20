//! `-p modules`: import/export linkage (DEBUG_PLAN §4.3).

use std::io::Write;

use varn_core::ast::{AstArena, Decl, Program, StmtKind};

use crate::fmt::Format;
use crate::render::{basename, BLUE, BOLD, CYAN, DIM, RESET, YELLOW};
use crate::report::Report;

/// Rows `[kind, a, b]` where kind is `"import"` (`[_, specifiers, source]`) or
/// `"export"` (`[_, repr]`).
pub fn collect(program: &Program, arena: &AstArena) -> Report {
    let mut rows = Vec::new();
    for &stmt in &program.body {
        if let StmtKind::Decl(decl) = &arena.stmt(stmt).kind {
            match &**decl {
                Decl::Import(i) => rows.push(vec![
                    "import".to_owned(),
                    format!("{:?}", i.specifiers),
                    format!("{:?}", i.source),
                ]),
                Decl::Export(e) => rows.push(vec!["export".to_owned(), format!("{:?}", e)]),
                _ => {}
            }
        }
    }
    Report::Rows(rows)
}

pub fn render(rep: &Report, filename: &str, fmt: Format, w: &mut dyn Write) -> std::io::Result<()> {
    let Report::Rows(rows) = rep else {
        return Ok(());
    };

    match fmt {
        Format::Plain => {
            let pad_len = (50_isize - "module linkage".len() as isize - 1).max(0) as usize;
            let padding = "─".repeat(pad_len);
            write!(
                w,
                "\n  {CYAN}module linkage{RESET} {DIM}{padding} {filename}{RESET}\n"
            )?;
            let mut imports = 0;
            let mut exports = 0;
            for r in rows {
                if r[0] == "import" {
                    write!(
                        w,
                        "  {BOLD}import{RESET} {YELLOW}{}{RESET} from {BLUE}{}{RESET}\n",
                        r[1], r[2]
                    )?;
                    imports += 1;
                } else {
                    write!(w, "  {BOLD}export{RESET} {CYAN}{}{RESET}\n", r[1])?;
                    exports += 1;
                }
            }
            let _ = (imports, exports);
            write!(w, "  {DIM}── end: module linkage ──{RESET}\n")?;
        }
        Format::Text => {
            writeln!(w, "# modules {}", basename(filename))?;
            writeln!(w, "kind|a|b")?;
            for r in rows {
                writeln!(w, "{}", r.join("|"))?;
            }
        }
    }
    Ok(())
}

pub fn debug_modules(program: &Program, arena: &AstArena) {
    let rep = collect(program, arena);
    let _ = render(
        &rep,
        &program.filename,
        Format::Plain,
        &mut std::io::stderr(),
    );
}
