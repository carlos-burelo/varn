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
fn line_col(source: &str, offset: u32) -> (u32, u32) {
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

/// One-line rendering of everything an annotation carries, or `-` when it
/// carries nothing. Fields are printed in a fixed order, and absent fields are
/// omitted rather than printed empty, so a diff line names what changed.
fn render_annotation(ann: &varn_core::ExprAnnotation) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(n) = ann.numeric {
        parts.push(format!("numeric={n:?}"));
    }
    if let Some(cg) = &ann.cg_ty {
        parts.push(format!("cg={cg:?}"));
    }
    if ann.type_only {
        parts.push("type_only".to_owned());
    }
    if ann.array_index {
        parts.push("array_index".to_owned());
    }
    if let Some(slot) = ann.slot_idx {
        parts.push(format!("slot={slot}"));
    }
    if let Some(slot) = ann.fixed_field_slot {
        parts.push(format!("fixed_field={slot}"));
    }
    if let Some(wire) = ann.intrinsic {
        parts.push(format!("intrinsic=0x{wire:02x}"));
    }
    if let Some(op) = ann.native_op {
        parts.push(format!("native_op={op}"));
    }
    if let Some(mapping) = &ann.call_mapping {
        parts.push(format!("call_mapping={mapping:?}"));
    }
    if parts.is_empty() {
        "-".to_owned()
    } else {
        parts.join(" ")
    }
}

/// The checker's type table and the codegen annotations for `program`, as
/// text.
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
        let _ = writeln!(out, "{id} | {}", entry.ty);
    }

    let _ = writeln!(out, "## annotations (key names its space)");
    let mut anns: Vec<(&varn_core::AnnKey, &varn_core::ExprAnnotation)> =
        check.type_annotations.entries().collect();
    // Declarations first, then expressions, each by their own number. The two
    // are different spaces, so there is no single ordering that mixes them
    // meaningfully — printing them apart is the honest rendering.
    anns.sort_by_key(|(k, _)| match k {
        varn_core::AnnKey::Decl(off) => (0u8, *off),
        varn_core::AnnKey::Expr(id) => (1u8, *id),
    });
    for (key, ann) in anns {
        let (label, num, offset) = match key {
            varn_core::AnnKey::Decl(off) => ("decl", *off, Some(*off)),
            varn_core::AnnKey::Expr(id) => ("expr", *id, check.expr_table.get(id).map(|e| e.start)),
        };
        let where_ = match offset {
            Some(off) => {
                let (line, col) = line_col(source, off);
                format!("{line}:{col}")
            }
            // An expression the checker annotated but never typed. Worth
            // seeing rather than hiding: it means the two passes disagree
            // about which nodes exist.
            None => "?:?".to_owned(),
        };
        let _ = writeln!(out, "{label} {num} | {where_} | {}", render_annotation(ann));
    }

    let _ = writeln!(out, "## reassigned names");
    let mut names: Vec<&str> = check.type_annotations.reassigned_names().collect();
    names.sort_unstable();
    let _ = writeln!(out, "{}", names.join(" "));

    out
}

/// `-p check:types`.
pub fn debug_check_types(program: &Program, source: &str, check: &CheckResult) {
    print!("{}", render_check_types(program, source, check));
}
