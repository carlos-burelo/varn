use varn_checker::CheckResult;
use varn_core::term::chalk::chalk;
use varn_core::term::terminal;
use varn_core::term::terminal::Section;

pub fn debug_symbols(
    check_result: &CheckResult,
    filename: &str,
    _flags: &crate::flags::DebugFlags,
) {
    Section::new("type inference engine")
        .subtitle(filename)
        .color(|c| c.blue())
        .print();

    terminal::log("  Symbol Types");
    terminal::log(format!(
        "  {}",
        chalk(format!(
            "{:<8} │ {:<15} │ {:<20} │ Type Details",
            "Loc", "Kind", "Name"
        ))
        .dim()
    ));
    terminal::log(format!("  {}", "─".repeat(80)));

    for (id, sym) in check_result.bind.arena.all().iter().enumerate() {
        let loc = format!(
            "{}:{}",
            sym.full_range.start.line + 1,
            sym.full_range.start.column
        );
        let kind_str = sym.kind.label().trim();
        let name = &sym.name;
        let ty = check_result
            .symbol_types
            .get(&id)
            .or(sym.ty.as_ref())
            .map(|t| t.to_string())
            .unwrap_or_else(|| "dynamic".to_string());

        let origin = sym.origin_module.as_deref().unwrap_or("");

        let is_core = origin.starts_with("core:")
            || origin.starts_with("builtin:")
            || (origin.is_empty() && sym.full_range.start.line == 0);

        let is_std = !is_core && origin.starts_with("std:");

        let tag_label = if is_core {
            "[core]"
        } else if is_std {
            "[std]"
        } else {
            "[usr]"
        };

        terminal::log(format!(
            "  {} │ {:<15} │ {} {} │ {}",
            chalk(format!("{:<8}", loc)).dim(),
            kind_str,
            chalk(tag_label).dim(),
            chalk(format!("{:<20}", name)).bold(),
            chalk(&ty).yellow()
        ));
    }

    terminal::blank();
    Section::new("type inference engine")
        .subtitle(format!(
            "{} symbols analyzed",
            check_result.bind.arena.len()
        ))
        .close();
}
