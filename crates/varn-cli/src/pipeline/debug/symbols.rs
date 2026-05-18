use varn_checker::CheckResult;
use crate::opts::DebugFlags;
use crate::pipeline::colors::{BOLD, DIM, RESET, YELLOW, BLUE, C_SYMBOLS};

pub fn debug_symbols(result: &CheckResult, filename: &str, flags: &DebugFlags) {
    use super::super::colors::{footer, header};
    header(C_SYMBOLS, "resolved symbols", filename);

    let mut local_count = 0;
    let mut std_count = 0;

    eprintln!("  {DIM}{:<20} │ {:<10} │ Type │ Line{RESET}", "Symbol", "Kind");
    eprintln!("  {}", "─".repeat(70));

    for sym in result.bind.arena.all() {
        if sym.origin_module.is_some() && !flags.symbols_all {
            std_count += 1;
            continue;
        }

        let loc = format!("{}", sym.line + 1);
        let ty_str = sym.ty.as_ref()
            .map(|t| t.to_string())
            .unwrap_or_else(|| "dynamic".to_owned());

        let name_prefix = if sym.origin_module.is_some() {
            format!("{DIM}[std]{RESET} ")
        } else {
            String::new()
        };

        eprintln!(
            "  {BOLD}{name_prefix}{:<20}{RESET} │ {YELLOW}{:<10}{RESET} │ {BLUE}{:<4}{RESET} │ {DIM}{loc}{RESET}",
            sym.name,
            sym.kind.label().trim(),
            ty_str,
            name_prefix = name_prefix,
            RESET = RESET, YELLOW = YELLOW, BLUE = BLUE, DIM = DIM, BOLD = BOLD
        );
        local_count += 1;
    }

    let msg = if std_count > 0 {
        format!("{local_count} symbols shown ({std_count} std symbols hidden, use symbols:all to show)")
    } else {
        format!("{local_count} symbols shown")
    };
    footer(C_SYMBOLS, &msg);
}
