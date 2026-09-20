//! Phase selection and filters — the data half of `DebugFlags`
//! (DEBUG_PLAN §3.3).
//!
//! `PhaseSel` is a `u64` bitflags set with one bit per registered phase; the
//! bit assignment is derived from [`crate::registry::ALL`], never hand-written.
//! `Filters` and `SubModes` carry the non-phase knobs the parser produces.

/// One bit per phase, stored densely. Empty means "no phase selected".
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

/// `--fn` and line-range filters, shared by the per-function dumps and the
/// line-oriented views.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Filters {
    pub fn_filter: Option<String>,
    pub line_range: Option<(u32, u32)>,
}

/// Sub-views a phase can be asked for with `phase:sub+sub`. Modeled as booleans
/// because several can be active at once (`clif:route+kinds`).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct SubModes {
    pub clif_route: bool,
    pub clif_kinds: bool,
    pub clif_ir: bool,
    pub clif_asm: bool,
    pub clif_check: bool,

    pub roots_diff: bool,
    pub roots_summary: bool,

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

/// Everything `-p` produced: which phases, which sub-views, which filters.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_selection_reports_empty() {
        assert!(Selection::default().is_empty());
    }

    #[test]
    fn bits_round_trip() {
        let mut s = PhaseSel::EMPTY;
        for b in [0u64, 3, 7, 63] {
            assert!(!s.contains(b));
            s.insert(b);
            assert!(s.contains(b));
        }
        assert!(!s.is_empty());
        s.remove(3);
        assert!(!s.contains(3));
        assert!(s.contains(7));
    }
}
