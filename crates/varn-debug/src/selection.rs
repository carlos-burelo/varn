#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct PhaseSel(u64);

impl PhaseSel {
    pub const EMPTY: Self = Self(0);

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn contains(self, bit: u64) -> bool {
        self.0 & (1 << bit) != 0
    }

    pub fn insert(&mut self, bit: u64) {
        self.0 |= 1 << bit;
    }

    pub fn remove(&mut self, bit: u64) {
        self.0 &= !(1 << bit);
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Filters {
    pub fn_filter: Option<String>,
    pub line_range: Option<(u32, u32)>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct SubModes {
    pub clif_route: bool,
    pub clif_kinds: bool,
    pub clif_ir: bool,
    pub clif_asm: bool,
    pub clif_check: bool,

    pub tir_check: bool,
    pub check_types: bool,
    pub symbols_all: bool,
    pub types_all: bool,

    pub lsp_hovers: bool,
    pub lsp_semantic: bool,
    pub lsp_types: bool,
    pub lsp_completions: bool,
    pub lsp_symbols: bool,
    pub lsp_colorize: bool,
    pub lsp_hints: bool,
}

#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Selection {
    pub sel: PhaseSel,
    pub sub: SubModes,
    pub filters: Filters,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.sel.is_empty()
    }
}
