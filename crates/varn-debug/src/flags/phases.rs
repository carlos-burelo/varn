use crate::phase::Stage;
use varn_core::term::terminal;
use varn_core::term::terminal::{Align, Section};

pub fn print_phases() {
    Section::new("debug phases")
        .subtitle("-p / --phase")
        .color(|c| c.cyan())
        .print();
    for (stage, label) in [
        (Stage::Lex, "lex"),
        (Stage::Parse, "parse"),
        (Stage::Check, "check"),
        (Stage::Compile, "compile"),
        (Stage::Exec, "exec"),
    ] {
        let phases: Vec<&'static dyn crate::phase::Phase> = crate::registry::ALL
            .iter()
            .copied()
            .filter(|p| p.stage() == stage)
            .collect();
        if phases.is_empty() {
            continue;
        }
        terminal::tagged(label, "");
        let mut table =
            terminal::Table::new(["phase", "description"]).align([Align::Left, Align::Left]);
        for p in phases {
            table.row([p.id().to_string(), p.title().to_string()]);
        }
        table.print();
    }
    let mut groups =
        terminal::Table::new(["phase", "description"]).align([Align::Left, Align::Left]);
    groups.row(["check".to_string(), "símbolos + tipos (grupo)".to_string()]);
    groups.row([
        "all".to_string(),
        "todas las fases con `in_all`".to_string(),
    ]);
    groups.row([
        "types:*".to_string(),
        "vistas del IDE (inspect_lsp)".to_string(),
    ]);
    groups.print();
    terminal::separator();
    terminal::tagged(
        "sub-fases",
        "check:types, tir:check, clif:route+kinds+ir+asm+check+all, lsp:*",
    );
    terminal::log(
        "  check:types  (volcado determinista y diffeable: tabla de tipos + anotaciones)",
    );
    terminal::log("  tir:check    (verifica el TIR emitido e informa cobertura sobre el módulo)");
    terminal::log("  clif:route  clif:kinds  clif:ir  clif:asm  clif:check  clif:all");
    terminal::log("  lsp:hovers  lsp:semantic  lsp:types  lsp:completions");
    terminal::log("  lsp:symbols  lsp:colorize  lsp:hints  lsp:all");
    terminal::separator();
    terminal::tagged("filtros", "--fn <nombre>, types:N, types:all, expr:N");
    terminal::log("  --fn <nombre>   limita los volcados por función a las que coincidan");
    terminal::log("  types:N  types:all  expr:N   rango de líneas");
    terminal::tagged(
        "env gc",
        "VARN_GC_TRACE=1   una línea por colección menor, según ocurre (cualquier comando)",
    );
    Section::new("debug phases").close();
}
