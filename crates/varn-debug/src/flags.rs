use crate::error::CliError;

#[derive(Clone, Default, Debug)]
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
    /// `check:types` — deterministic, diffable dump of the checker's type
    /// table and the annotations that reach codegen. The baseline the checker
    /// refactor is verified against, so it is deliberately NOT part of
    /// `check` or `all`: those are for reading, this one is for diffing.
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

    pub hir: bool,

    pub ssa: bool,
    pub suspend: bool,

    pub clif: bool,
    pub clif_route: bool,
    pub clif_kinds: bool,
    pub clif_ir: bool,
    pub clif_asm: bool,

    pub tiers: bool,
    pub bails: bool,
    pub summary: bool,

    pub roots: bool,
    /// Only safepoints where the two root answers disagree.
    pub roots_diff: bool,
    /// Counts only, no per-safepoint rows.
    pub roots_summary: bool,

    /// Substring filter applied to function names by the per-function dumps.
    /// Without it, `-p bytecode` on a real module prints every function.
    pub fn_filter: Option<String>,
}

/// Every phase name `-p` accepts, with a one-line description. Single source
/// for `--list-phases` and for the error message on an unknown phase.
pub const PHASES: &[(&str, &str)] = &[
    ("tokens", "flujo de tokens del lexer"),
    ("ast", "árbol sintáctico"),
    ("check", "símbolos, binds, tipos y expresiones"),
    ("bytecode", "bytecode por función"),
    ("hir", "HIR previo a SSA"),
    ("ssa", "forma SSA"),
    (
        "suspend",
        "puntos de suspensión Await/Yield con su conjunto vivo, in_try, in_loop",
    ),
    ("clif", "lowering Cranelift (route, kinds, ir, asm)"),
    ("tiers", "tier por función: clif / gate / bail"),
    (
        "roots",
        "conjunto de raíces GC por safepoint, contra la liveness de Cranelift",
    ),
    ("bails", "solo lo que no rutea, agrupado por causa"),
    ("summary", "tamaños, exports y top-10 de funciones"),
    ("graph", "grafo de módulos"),
    ("caps", "traza de capabilities"),
    ("info", "metadatos del módulo"),
    ("all", "todo lo anterior"),
];

pub fn print_phases() {
    eprintln!("Fases disponibles para -p / --phase:\n");
    for (name, desc) in PHASES {
        eprintln!("  {name:<10} {desc}");
    }
    eprintln!("\nSub-fases:");
    eprintln!("  check:types  (volcado determinista y diffeable: tabla de tipos + anotaciones)");
    eprintln!("  clif:route  clif:kinds  clif:ir  clif:asm  clif:all");
    eprintln!("  roots:diff  roots:summary  roots:all");
    eprintln!("  lsp:hovers  lsp:semantic  lsp:types  lsp:completions");
    eprintln!("  lsp:symbols  lsp:colorize  lsp:hints  lsp:all");
    eprintln!("\nFiltros:");
    eprintln!("  --fn <nombre>   limita los volcados por función a las que coincidan");
    eprintln!("  types:N  types:all  expr:N   rango de líneas");
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
            if let Some(sub) = phase.strip_prefix("check:") {
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
            } else if let Some(sub) = phase.strip_prefix("roots:") {
                flags.roots = true;
                for sub_part in sub.split('+') {
                    match sub_part {
                        "diff" => flags.roots_diff = true,
                        "summary" => flags.roots_summary = true,
                        "all" => {}
                        unknown => {
                            return Err(CliError::usage(format!(
                                "unknown roots debug sub-phase: '{unknown}'\n\
                                 Valid sub-phases: diff, summary, all"
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
                        "all" => flags.clif_all(),
                        unknown => {
                            return Err(CliError::usage(format!(
                                "unknown clif debug sub-phase: '{unknown}'\n\
                                 Valid sub-phases: route, kinds, ir, asm, all"
                            )));
                        }
                    }
                }
            } else {
                match phase {
                    "tokens" => flags.tokens = true,
                    "ast" => flags.ast = true,
                    "bytecode" => flags.bytecode = true,
                    "symbols" => flags.symbols = true,
                    "binds" => flags.binds = true,
                    "modules" => flags.modules = true,
                    "types" => flags.types = true,
                    "expr" => flags.expr = true,
                    "errors" => flags.errors = true,
                    "trace" => flags.trace = true,
                    "calls" => flags.calls = true,
                    "consts" => flags.consts = true,
                    "scope" => flags.scope = true,
                    "graph" => flags.graph = true,
                    "cap-trace" | "cap" | "caps" => flags.cap_trace = true,
                    "check" => {
                        flags.symbols = true;
                        flags.symbols_all = true;
                        flags.binds = true;
                        flags.types = true;
                        flags.types_all = true;
                        flags.expr = true;
                    }
                    "info" => flags.info = true,
                    "lsp" => flags.lsp = true,
                    "hir" => flags.hir = true,
                    "ssa" => flags.ssa = true,
                    "suspend" => flags.suspend = true,
                    "tiers" => flags.tiers = true,
                    "bails" => flags.bails = true,
                    "roots" => flags.roots = true,
                    "summary" => flags.summary = true,
                    "clif" => flags.clif_all_on(),
                    "all" => {
                        flags.tokens = true;
                        flags.ast = true;
                        flags.bytecode = true;
                        flags.symbols = true;
                        flags.symbols_all = true;
                        flags.modules = true;
                        flags.types = true;
                        flags.types_all = true;
                        flags.expr = true;
                        flags.scope = true;
                        flags.info = true;
                        flags.graph = true;
                        flags.cap_trace = true;
                        flags.lsp = true;
                        flags.lsp_all();
                        flags.hir = true;
                        flags.ssa = true;
                        flags.tiers = true;
                        flags.bails = true;
                        flags.summary = true;
                        flags.clif_all_on();
                    }
                    unknown => {
                        let names: Vec<&str> = PHASES.iter().map(|(n, _)| *n).collect();
                        return Err(CliError::usage(format!(
                            "unknown debug phase: '{unknown}'\n\
                             Valid phases: {}\n\
                             Run with --list-phases for descriptions.",
                            names.join(", ")
                        )));
                    }
                }
            }
        }
        Ok(flags)
    }

    pub fn any(&self) -> bool {
        self.tokens
            || self.ast
            || self.bytecode
            || self.symbols
            || self.binds
            || self.modules
            || self.types
            || self.expr
            || self.errors
            || self.trace
            || self.calls
            || self.consts
            || self.scope
            || self.graph
            || self.cap_trace
            || self.info
            || self.lsp
            || self.hir
            || self.ssa
            || self.suspend
            || self.clif
            || self.tiers
            || self.bails
            || self.summary
            || self.roots
            || self.check_types
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

    /// Bare `clif` = the phase plus all four views.
    pub fn clif_all_on(&mut self) {
        self.clif = true;
        self.clif_all();
    }
}
