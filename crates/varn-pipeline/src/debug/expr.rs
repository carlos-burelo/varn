use crate::colors::{BOLD, C_TYPES, DIM, RESET, YELLOW};
use varn_core::ast::Program;

pub fn debug_expr(program: &Program, range: Option<(u32, u32)>) {
    use super::colors::{footer, header};

    let result = varn_checker::Checker::check(program);
    if result.expr_types.is_empty() {
        return;
    }

    header(C_TYPES, "expression inference map", &program.filename);

    let src = std::fs::read_to_string(&program.filename).unwrap_or_default();
    let src_bytes = src.as_bytes();
    let mut line_starts = vec![0];
    for (i, &b) in src_bytes.iter().enumerate() {
        if b == b'\n' {
            line_starts.push(i + 1);
        }
    }

    let offset_to_linecol = |off: u32| -> (u32, u32) {
        let off = off as usize;
        let line_idx = line_starts.partition_point(|&s| s <= off).saturating_sub(1);
        let col = off - line_starts[line_idx];
        ((line_idx + 1) as u32, (col + 1) as u32)
    };

    let in_range = |line: u32| match range {
        Some((lo, hi)) => line >= lo && line <= hi,
        None => true,
    };

    let mut sorted_exprs: Vec<_> = result.expr_types.iter().collect();
    sorted_exprs.sort_by_key(|(off, _)| *off);

    eprintln!(
        "  {DIM}{:<8} │ {:<30} │ Inferred Type{RESET}",
        "Loc", "Snippet"
    );
    eprintln!("  {}", "─".repeat(80));

    let mut shown = 0;
    for (offset, info) in sorted_exprs {
        let (line, col) = offset_to_linecol(*offset);
        if !in_range(line) {
            continue;
        }

        let start = *offset as usize;
        let raw_end = src_bytes[start..]
            .iter()
            .position(|&b| b == b'\n' || b == b'\r')
            .unwrap_or(src_bytes.len() - start)
            .min(30);
        let snip = String::from_utf8_lossy(&src_bytes[start..start + raw_end]);

        eprintln!(
            "  {DIM}{:<3}:{:>3}{RESET} │ {YELLOW}{:<30}{RESET} │ {BOLD}{}{RESET}",
            line,
            col,
            snip.trim(),
            info.ty
        );
        shown += 1;
    }

    footer(C_TYPES, &format!("{} expressions analyzed", shown));
}
