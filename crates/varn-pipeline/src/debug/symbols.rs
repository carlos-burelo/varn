use varn_checker::CheckResult;
use varn_debug::flags::DebugFlags;
use varn_term::chalk::chalk;
use varn_term::terminal::{Section, Table};

pub fn debug_symbols(result: &CheckResult, filename: &str, flags: &DebugFlags) {
    Section::new("resolved symbols").subtitle(filename).color(|c| c.blue()).print();

    let mut table = Table::new(["Symbol", "Kind", "Type", "Line"]);

    let mut local_count = 0;
    let mut std_count = 0;

    for sym in result.bind.arena.all() {
        if sym.origin_module.is_some() && !flags.symbols_all {
            std_count += 1;
            continue;
        }

        let name_prefix = if sym.origin_module.is_some() {
            format!("{} ", chalk("[std]").dim())
        } else {
            String::new()
        };

        table.row([
            format!("{}{}", name_prefix, chalk(&sym.name).bold()),
            chalk(sym.kind.label().trim()).yellow().to_string(),
            chalk(
                sym.ty.as_ref()
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "dynamic".to_owned()),
            ).blue().to_string(),
            chalk(sym.line + 1).dim().to_string(),
        ]);
        local_count += 1;
    }
    table.print();

    let msg = if std_count > 0 {
        format!("{local_count} symbols shown ({std_count} std symbols hidden, use symbols:all to show)")
    } else {
        format!("{local_count} symbols shown")
    };
    varn_term::terminal::log(chalk(format!("── {msg} ──")).dim());
}
