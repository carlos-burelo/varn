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
    pub lsp_interact: bool,
    pub lsp_cursors: Vec<Step>,

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

use super::steps::Step;

impl DebugFlags {
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
        self.lsp_interact = true;
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
