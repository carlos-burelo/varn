use crate::colors::{BOLD, C_TYPES, DIM, R, RESET, YELLOW};
use varn_checker::CheckResult;

pub fn debug_symbols(
    check_result: &CheckResult,
    filename: &str,
    _flags: &crate::flags::DebugFlags,
) {
    use crate::colors::{footer, header};
    header(C_TYPES, "type inference engine", filename);

    eprintln!("  Symbol Types");
    eprintln!(
        "  {DIM}{:<8} │ {:<15} │ {:<20} │ Type Details{RESET}",
        "Loc",
        "Kind",
        "Name",
        DIM = DIM,
        RESET = R
    );
    eprintln!("  {}", "─".repeat(80));

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
            .or_else(|| sym.ty.as_ref())
            .map(|t| t.to_string())
            .unwrap_or_else(|| "dynamic".to_string());

        let origin = sym.origin_module.as_deref().unwrap_or("");

        let is_core = origin.starts_with("core:")
            || origin.starts_with("builtin:")
            || origin.contains("varn-stdlib/builtins")
            || origin.contains(r"varn-stdlib\builtins")
            || (origin.is_empty() && sym.full_range.start.line == 0);

        let is_std = !is_core
            && (origin.starts_with("std:")
                || origin.contains("varn-stdlib/")
                || origin.contains(r"varn-stdlib\"));

        let tag = if is_core {
            format!("{DIM}[core]{RESET} ")
        } else if is_std {
            format!("{DIM}[std]{RESET} ")
        } else {
            format!("{DIM}[usr]{RESET} ")
        };

        eprintln!(
            "  {DIM}{:<8}{RESET} │ {:<15} │ {BOLD}{tag}{:<20}{RESET} │ {YELLOW}{}{RESET}",
            loc,
            kind_str,
            name,
            ty,
            tag = tag,
            DIM = DIM,
            RESET = R,
            BOLD = BOLD,
            YELLOW = YELLOW
        );
    }

    eprintln!();
    footer(
        C_TYPES,
        &format!("{} symbols analyzed", check_result.bind.arena.len()),
    );
}
