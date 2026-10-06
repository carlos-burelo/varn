use crate::error::CliError;

#[derive(Clone, Default, Debug, PartialEq)]
pub struct DebugFlags {
    pub tokens: bool,
    pub ast: bool,
    pub bytecode: bool,
    pub symbols: bool,
    pub symbols_all: bool,
    pub binds: bool,
    pub modules: bool,
    pub types: bool,
    pub types_all: bool,
    pub types_range: Option<(u32, u32)>,
    pub expr: bool,
    pub expr_range: Option<(u32, u32)>,

    pub check_types: bool,
    pub errors: bool,
    pub trace: bool,
    pub calls: bool,
    pub consts: bool,
    pub scope: bool,
    pub graph: bool,
    pub cap_trace: bool,
    pub info: bool,
    pub lsp: bool,
    pub lsp_hovers: bool,
    pub lsp_semantic: bool,
    pub lsp_types: bool,
    pub lsp_completions: bool,
    pub lsp_symbols: bool,
    pub lsp_colorize: bool,
    pub lsp_hints: bool,

    pub tir: bool,
    pub tir_check: bool,

    pub clif: bool,
    pub clif_route: bool,
    pub clif_kinds: bool,
    pub clif_ir: bool,
    pub clif_asm: bool,

    pub clif_check: bool,

    pub tiers: bool,
    pub bails: bool,
    pub summary: bool,

    pub typeloss: bool,

    pub gc: bool,

    pub fn_filter: Option<String>,
}

fn valid_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = crate::registry::ALL.iter().map(|p| p.id()).collect();
    names.extend(["check", "all", "types", "lsp", "trace"]);
    names
}

pub fn print_phases() {
    use crate::phase::Stage;
    use varn_core::term::terminal;
    use varn_core::term::terminal::{Align, Section};

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

pub fn parse_line_range(s: &str) -> Result<(u32, u32), CliError> {
    if s == "all" {
        return Ok((0, u32::MAX));
    }
    if let Some((lo_str, hi_str)) = s.split_once('-') {
        let lo = lo_str.parse::<u32>().map_err(|_| {
            CliError::usage(format!(
                "invalid line range: '{s}' (expected N, N-M, or N-)"
            ))
        })?;
        let hi = if hi_str.is_empty() {
            u32::MAX
        } else {
            hi_str.parse::<u32>().map_err(|_| {
                CliError::usage(format!(
                    "invalid line range: '{s}' (expected N, N-M, or N-)"
                ))
            })?
        };
        Ok((lo, hi))
    } else {
        let line = s
            .parse::<u32>()
            .map_err(|_| CliError::usage(format!("invalid line number: '{s}'")))?;
        Ok((line, line))
    }
}

impl DebugFlags {
    pub fn parse(spec: &str) -> Result<Self, CliError> {
        let mut flags = DebugFlags::default();
        for part in spec.split(',') {
            let phase = part.trim();
            if phase.is_empty() {
                continue;
            }
            if let Some(sub) = phase.strip_prefix("tir:") {
                match sub {
                    "check" => flags.tir_check = true,
                    unknown => {
                        return Err(CliError::usage(format!(
                            "unknown tir debug sub-phase: '{unknown}'\n\
                             Valid sub-phases: check"
                        )));
                    }
                }
            } else if let Some(sub) = phase.strip_prefix("check:") {
                match sub {
                    "types" => flags.check_types = true,
                    unknown => {
                        return Err(CliError::usage(format!(
                            "unknown check debug sub-phase: '{unknown}'\n\
                             Valid sub-phases: types"
                        )));
                    }
                }
            } else if let Some(range_str) = phase.strip_prefix("types:") {
                flags.types = true;
                if range_str == "all" {
                    flags.types_all = true;
                } else {
                    flags.types_range = Some(parse_line_range(range_str)?);
                }
            } else if let Some(range_str) = phase.strip_prefix("symbols:") {
                flags.symbols = true;
                if range_str == "all" {
                    flags.symbols_all = true;
                }
            } else if let Some(range_str) = phase.strip_prefix("expr:") {
                flags.expr = true;
                flags.expr_range = Some(parse_line_range(range_str)?);
            } else if let Some(sub) = phase.strip_prefix("lsp:") {
                flags.lsp = true;
                for sub_part in sub.split('+') {
                    match sub_part {
                        "hovers" => flags.lsp_hovers = true,
                        "semantic" => flags.lsp_semantic = true,
                        "types" => flags.lsp_types = true,
                        "completions" => flags.lsp_completions = true,
                        "symbols" => flags.lsp_symbols = true,
                        "colorize" => flags.lsp_colorize = true,
                        "hints" => flags.lsp_hints = true,
                        "all" => flags.lsp_all(),
                        unknown => {
                            return Err(CliError::usage(format!(
                                "unknown lsp debug sub-phase: '{unknown}'\n\
                                 Valid sub-phases: hovers, semantic, types, completions, symbols, colorize, hints, all"
                            )));
                        }
                    }
                }
            } else if let Some(sub) = phase.strip_prefix("clif:") {
                flags.clif = true;
                for sub_part in sub.split('+') {
                    match sub_part {
                        "route" => flags.clif_route = true,
                        "kinds" => flags.clif_kinds = true,
                        "ir" => flags.clif_ir = true,
                        "asm" => flags.clif_asm = true,
                        "check" => flags.clif_check = true,
                        "all" => flags.clif_all(),
                        unknown => {
                            return Err(CliError::usage(format!(
                                "unknown clif debug sub-phase: '{unknown}'\n\
                                 Valid sub-phases: route, kinds, ir, asm, check, all"
                            )));
                        }
                    }
                }
            } else {
                match phase {
                    "check" => {
                        flags.symbols = true;
                        flags.symbols_all = true;
                        flags.types = true;
                        flags.types_all = true;
                    }

                    "lsp" => flags.lsp = true,
                    "types" => flags.types = true,

                    "trace" => flags.trace = true,

                    "all" => {
                        for p in crate::registry::in_all() {
                            apply_registered(&mut flags, p.id());
                        }
                        flags.symbols_all = true;
                        flags.types = true;
                        flags.types_all = true;
                        flags.lsp = true;
                        flags.lsp_all();
                    }

                    "binds" => flags.binds = true,
                    "expr" => flags.expr = true,
                    "errors" => flags.errors = true,
                    "calls" => flags.calls = true,
                    "consts" => flags.consts = true,
                    "info" => flags.info = true,
                    unknown => match crate::registry::lookup(unknown) {
                        Some(p) => apply_registered(&mut flags, p.id()),
                        None => {
                            let names = valid_names();
                            return Err(CliError::usage(format!(
                                "unknown debug phase: '{unknown}'\n\
                                 Valid phases: {}\n\
                                 Run with --list-phases for descriptions.",
                                names.join(", ")
                            )));
                        }
                    },
                }
            }
        }
        Ok(flags)
    }

    pub fn needs_execution(&self) -> bool {
        self.gc
    }

    pub fn any(&self) -> bool {
        *self != Self::default()
    }

    pub fn lsp_all(&mut self) {
        self.lsp_hovers = true;
        self.lsp_semantic = true;
        self.lsp_types = true;
        self.lsp_completions = true;
        self.lsp_symbols = true;
        self.lsp_colorize = true;
        self.lsp_hints = true;
    }

    pub fn clif_all(&mut self) {
        self.clif_route = true;
        self.clif_kinds = true;
        self.clif_ir = true;
        self.clif_asm = true;
    }

    pub fn clif_all_on(&mut self) {
        self.clif = true;
        self.clif_all();
    }
}

fn apply_registered(flags: &mut DebugFlags, id: &str) {
    match id {
        "tokens" => flags.tokens = true,
        "ast" => flags.ast = true,
        "modules" => flags.modules = true,
        "symbols" => flags.symbols = true,
        "check:types" => flags.check_types = true,
        "bytecode" => flags.bytecode = true,
        "scope" => flags.scope = true,
        "caps" => flags.cap_trace = true,
        "graph" => flags.graph = true,
        "summary" => flags.summary = true,
        "typeloss" => flags.typeloss = true,
        "tir" => flags.tir = true,
        "tiers" => flags.tiers = true,
        "bails" => flags.bails = true,
        "clif" => flags.clif_all_on(),
        "gc" => flags.gc = true,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_phase_sets_its_flag() {
        assert!(DebugFlags::parse("tokens").unwrap().tokens);
        assert!(DebugFlags::parse("symbols").unwrap().symbols);
        assert!(DebugFlags::parse("modules").unwrap().modules);
        assert!(DebugFlags::parse("scope").unwrap().scope);
        assert!(DebugFlags::parse("check:types").unwrap().check_types);
        assert!(DebugFlags::parse("cap").unwrap().cap_trace);
        assert!(DebugFlags::parse("cap-trace").unwrap().cap_trace);
        assert!(DebugFlags::parse("caps").unwrap().cap_trace);
    }

    #[test]
    fn clif_bare_is_the_phase_plus_views_without_check() {
        let f = DebugFlags::parse("clif").unwrap();
        assert!(f.clif && f.clif_route && f.clif_kinds && f.clif_ir && f.clif_asm);
        assert!(!f.clif_check);
    }

    #[test]
    fn clif_all_excludes_check() {
        let f = DebugFlags::parse("clif:all").unwrap();
        assert!(f.clif_route && f.clif_kinds && f.clif_ir && f.clif_asm);
        assert!(!f.clif_check);
    }

    #[test]
    fn check_group_is_symbols_and_types_not_check_types() {
        let f = DebugFlags::parse("check").unwrap();
        assert!(f.symbols && f.symbols_all && f.types && f.types_all);
        assert!(!f.check_types);
        assert!(!f.binds && !f.expr);
    }

    #[test]
    fn all_is_derived_and_excludes_sweep_phases() {
        let f = DebugFlags::parse("all").unwrap();
        for on in [
            f.tokens,
            f.ast,
            f.bytecode,
            f.symbols,
            f.modules,
            f.scope,
            f.graph,
            f.cap_trace,
            f.tiers,
            f.bails,
            f.summary,
            f.clif,
            f.types,
            f.lsp,
        ] {
            assert!(on);
        }
        assert!(f.symbols_all && f.types_all && f.lsp_hovers);
        for off in [f.typeloss, f.gc, f.tir, f.check_types] {
            assert!(!off);
        }
    }

    #[test]
    fn submodes_parse() {
        let f = DebugFlags::parse("tir:check").unwrap();
        assert!(f.tir_check);
        let f = DebugFlags::parse("symbols:all").unwrap();
        assert!(f.symbols && f.symbols_all);
    }

    #[test]
    fn unknown_and_bad_subphases_error() {
        assert!(DebugFlags::parse("definitely-not-a-phase").is_err());
        assert!(DebugFlags::parse("tir:nope").is_err());
        assert!(DebugFlags::parse("clif:nope").is_err());
    }

    #[test]
    fn any_and_needs_execution() {
        assert!(!DebugFlags::parse("").unwrap().any());
        assert!(DebugFlags::parse("tokens").unwrap().any());
        assert!(!DebugFlags::parse("tokens").unwrap().needs_execution());
        assert!(DebugFlags::parse("gc").unwrap().needs_execution());
    }

    #[test]
    fn valid_names_covers_registry_and_groups() {
        let names = super::valid_names();
        for p in crate::registry::ALL {
            assert!(names.contains(&p.id()), "missing {}", p.id());
        }
        for g in ["check", "all", "types", "lsp", "trace"] {
            assert!(names.contains(&g), "missing {g}");
        }
    }
}
