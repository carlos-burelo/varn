use varn_checker::SymbolKind;
use varn_core::ast::Program;
use varn_debug::flags::DebugFlags;
use crate::colors::{BOLD, DIM, RESET, YELLOW, BLUE, C_TYPES};

pub fn debug_types(program: &Program, flags: &DebugFlags) {
    use super::colors::{footer, header};
    
    let source_code = std::fs::read_to_string(&program.filename).unwrap_or_default();
    let uri = varn_modules::resolver::path_to_uri(&program.filename);

    let analysis = varn_lsp::pipeline::run_pipeline(source_code, uri);
    header(C_TYPES, "type inference engine", &program.filename);

    let in_range = |line: u32| match flags.types_range {
        Some((lo, hi)) => line >= lo && line <= hi,
        None => true,
    };

    let mut entries = Vec::new();
    let mut std_hidden = 0;

    for sym in &analysis.symbols {
        let show_std = flags.types_all;
        if (show_std || !sym.is_from_stdlib) && in_range(sym.line + 1) {
            entries.push(sym.clone());
        } else if sym.is_from_stdlib {
            std_hidden += 1;
        }
    }

    if !entries.is_empty() {
        eprintln!("  {BOLD}{BLUE}Symbol Types{RESET}");
        eprintln!("  {DIM}{:<8} │ {:<15} │ {:<20} │ Type Details{RESET}", "Loc", "Kind", "Name");
        eprintln!("  {}", "─".repeat(80));

        for s in entries {
            let loc = format!("{}:{}", s.line + 1, s.col + 1);
            let kind = s.kind.label();
            let details = format_sym_details(&s);
            
            let name_prefix = if s.is_from_stdlib {
                format!("{DIM}[std]{RESET} ")
            } else {
                String::new()
            };

            eprintln!(
                "  {DIM}{:<8}{RESET} │ {YELLOW}{:<15}{RESET} │ {BOLD}{name_prefix}{:<20}{RESET} │ {DIM}{details}{RESET}",
                loc, kind, s.name, name_prefix = name_prefix, details = details,
                DIM = DIM, RESET = RESET, YELLOW = YELLOW, BOLD = BOLD
            );
        }
        eprintln!();
    }

    let footer_msg = if std_hidden > 0 {
        format!("{} symbols analyzed ({} std symbols hidden, use types:all to show)", analysis.symbols.len(), std_hidden)
    } else {
        format!("{} symbols analyzed", analysis.symbols.len())
    };
    footer(C_TYPES, &footer_msg);
}

fn format_sym_details(s: &varn_lsp::document::SymbolRecord) -> String {
    match s.kind {
        SymbolKind::Function => {
            let async_prefix = if s.is_async { "async " } else { "" };
            format!("{async_prefix}{}", s.ty)
        }
        SymbolKind::Class | SymbolKind::Interface => {
            let mut parts = Vec::new();
            if !s.type_params.is_empty() {
                parts.push(format!("<{}>", s.type_params.join(", ")));
            }
            if !s.members.is_empty() {
                parts.push(format!("{{ {} members }}", s.members.len()));
            }
            parts.join(" ")
        }
        _ => s.ty.to_string(),
    }
}
