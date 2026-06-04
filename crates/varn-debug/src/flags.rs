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
            if let Some(range_str) = phase.strip_prefix("types:") {
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
                    }
                    unknown => {
                        return Err(CliError::usage(format!(
                            "unknown debug phase: '{unknown}'\n\
                             Valid phases: tokens, ast, check, bytecode, graph, caps, info, all\n\
                             LSP sub-phases: lsp:hovers, lsp:semantic, lsp:types, lsp:completions, lsp:symbols, lsp:colorize, lsp:hints, lsp:all\n\
                             Line range filter: types:N  types:all  expr:N"
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
}
