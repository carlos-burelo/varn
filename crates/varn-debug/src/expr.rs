//! `-p check:types` — the checker's answers, in a form a diff can read.
//!
//! This exists to make a refactor of `varn-checker` verifiable. The checker
//! currently answers "what is the type of this expression" from three
//! different engines (see the plan in `docs/`), and collapsing them to one is
//! only safe if every type it produces today can be compared against every
//! type it produces tomorrow.
//!
//! So the requirements here are not the usual debug-dump ones:
//!
//! * **Deterministic.** Everything is emitted in sorted key order. `-p check`
//!   prints symbols in `HashMap` iteration order, which differs run to run and
//!   is therefore useless as a baseline.
//! * **Machine-diffable.** One record per line, `|`-separated, no colour, no
//!   box drawing, no elapsed times.
//! * **Both sides.** The checker's own table AND the annotations that reach
//!   codegen, because the whole point is that those two can disagree.

use std::fmt::Write as _;

use varn_checker::CheckResult;
use varn_core::ast::Program;

/// Line and column (1-based) of a byte offset in `source`.
///
/// Recomputed per call rather than through an index: this runs only under a
/// debug flag, over one file, and a wrong line number in a baseline is worse
/// than a slow one.
pub(crate) fn line_col(source: &str, offset: u32) -> (u32, u32) {
    let offset = offset as usize;
    let mut line = 1u32;
    let mut col = 1u32;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// The checker's type table for `program`, as text.
///
/// Returns a `String` rather than printing, so the golden test and the
/// `-p check:types` flag render the *same* bytes. A dump that only prints can
/// be read but not asserted on, which is how `-p check` ended up unusable as a
/// baseline.
///
/// The two tables are separate sections, and that is the finding rather than a
/// formatting choice: the checker's table is keyed by `Expr::id()` while the
/// annotations are keyed by byte offset, so they cannot be joined into one
/// row. Collapsing the two key spaces is Phase 1 of the plan; until then a
/// baseline has to show both.
pub fn render_check_types(program: &Program, source: &str, check: &CheckResult) -> String {
    let mut out = String::new();

    // Basename only. `program.filename` is an absolute path — with a `\\?\`
    // prefix on Windows — and a baseline that embeds one checkout's directory
    // layout cannot be committed or compared across machines.
    let name = program
        .filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&program.filename);
    let _ = writeln!(out, "# check:types {name}");

    let _ = writeln!(out, "## checker types (key = expr id)");
    let mut by_id: Vec<(&u32, &varn_checker::TypeEntry)> = check.expr_table.iter().collect();
    by_id.sort_by_key(|(id, _)| **id);
    for (id, entry) in by_id {
        let (line, col) = line_col(source, entry.start);
        let ty = entry.ty.display(&check.bind.ty_table, &check.bind.interner);
        let _ = writeln!(out, "{id} | {line}:{col} | {ty}");
    }

    out
}

/// `-p check:types` — migrated to `phases::check_types` (collect/render).
pub use crate::phases::check_types::debug_check_types;
