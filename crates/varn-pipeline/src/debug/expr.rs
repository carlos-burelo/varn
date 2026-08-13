use varn_core::ast::Program;
use varn_term::chalk::chalk;
use varn_term::terminal::Section;

pub fn debug_expr(program: &Program, range: Option<(u32, u32)>) {
    let result = varn_checker::Checker::check(program);
    if result.expr_types.is_empty() {
        return;
    }

    Section::new("expression inference map").subtitle(&program.filename).color(|c| c.blue()).print();

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

    varn_term::terminal::log(format!(
        "  {}",
        chalk(format!("{:<8} │ {:<30} │ Inferred Type", "Loc", "Snippet")).dim()
    ));
    varn_term::terminal::log(format!("  {}", "─".repeat(80)));

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

        varn_term::terminal::log(format!(
            "  {} │ {} │ {}",
            chalk(format!("{:<3}:{:>3}", line, col)).dim(),
            chalk(format!("{:<30}", snip.trim())).yellow(),
            chalk(&info.ty).bold(),
        ));
        shown += 1;
    }

    varn_term::terminal::log(chalk(format!("── {shown} expressions analyzed ──")).dim());
}
