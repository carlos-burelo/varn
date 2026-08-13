use varn_checker::SymbolKind;
use varn_core::ast::Program;
use varn_debug::flags::DebugFlags;
use varn_term::chalk::chalk;
use varn_term::terminal::{Section, Table};

pub fn debug_types(program: &Program, flags: &DebugFlags) {
    let source_code = std::fs::read_to_string(&program.filename).unwrap_or_default();
    let uri = varn_modules::resolver::path_to_uri(&program.filename);

    let analysis = varn_lsp::pipeline::run_pipeline(source_code, uri);
    Section::new("type inference engine").subtitle(&program.filename).color(|c| c.blue()).print();

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
        let mut table = Table::new(["Loc", "Kind", "Name", "Type Details"]);

        for s in entries {
            let loc = format!("{}:{}", s.line + 1, s.col + 1);
            let kind = s.kind.label();
            let details = format_sym_details(&s);

            let name_prefix = if s.is_from_stdlib {
                format!("{} ", chalk("[std]").dim())
            } else {
                String::new()
            };

            table.row([
                chalk(loc).dim().to_string(),
                chalk(kind).yellow().to_string(),
                format!("{}{}", name_prefix, chalk(&s.name).bold()),
                chalk(details).dim().to_string(),
            ]);
        }
        table.print();
        varn_term::terminal::blank();
    }

    let footer_msg = if std_hidden > 0 {
        format!("{} symbols analyzed ({} std symbols hidden, use types:all to show)", analysis.symbols.len(), std_hidden)
    } else {
        format!("{} symbols analyzed", analysis.symbols.len())
    };
    varn_term::terminal::log(chalk(format!("── {footer_msg} ──")).dim());
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
