//! `vn debug -p tiers` and `-p bails` — which functions reach Cranelift, and
//! what stops the rest.
//!
//! This is a *static* view: it runs the real lowering via
//! [`varn_jit::clif::debug::inspect`] without executing the program, so it
//! answers "would this route" rather than "did this run".
//!
//! The size gate is applied here explicitly. It lives in `varn_jit::compile`,
//! outside `try_compile`, so `inspect` alone would happily report a 400-word
//! function as routed when production never offers it to Cranelift at all.

use varn_jit::clif::debug::inspect;
use varn_jit::clif::lower::NoLinker;
use varn_jit::{JitHelpers, SIZE_GATE_WORDS};
use varn_types::{FunctionProto, Literal, PoolEntry, VmValue};

use crate::flags::DebugFlags;

const BOLD: &str = "\x1b[1m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";
const R: &str = "\x1b[0m";

/// Why a function is not compiled, in the order production decides it.
#[derive(PartialEq, Eq)]
pub enum Tier {
    Clif,
    /// Refused before Cranelift was asked.
    Gate(String),
    /// Cranelift was asked and refused.
    Bail(String),
}

pub struct TierRow {
    pub name: String,
    pub words: usize,
    pub tier: Tier,
    pub frame_aware: bool,
    /// Which tests made it frame-aware — see `clif::lower::frame_aware_reasons`.
    pub fa_reasons: Vec<&'static str>,
}

impl TierRow {
    fn marker(&self) -> &'static str {
        match self.tier {
            Tier::Clif => "clif",
            Tier::Gate(_) => "gate",
            Tier::Bail(_) => "bail",
        }
    }

    fn colour(&self) -> &'static str {
        match self.tier {
            Tier::Clif => GREEN,
            Tier::Gate(_) => YELLOW,
            Tier::Bail(_) => RED,
        }
    }

    fn reason(&self) -> &str {
        match &self.tier {
            Tier::Clif => "",
            Tier::Gate(r) | Tier::Bail(r) => r,
        }
    }
}

/// Classify `proto` and every nested function proto.
pub fn classify(proto: &FunctionProto, helpers: &JitHelpers) -> Vec<TierRow> {
    let Ok(isa) = varn_jit::clif::shared_isa() else {
        return Vec::new();
    };
    let resolved = crate::resolved_copy(proto);
    let mut rows = Vec::new();
    walk(&resolved, helpers, isa, &mut rows);
    rows
}

fn walk(
    proto: &FunctionProto,
    helpers: &JitHelpers,
    isa: &varn_jit::OwnedTargetIsa,
    out: &mut Vec<TierRow>,
) {
    let name = proto.name.as_deref().unwrap_or("<module>").to_owned();
    let words = proto.chunk.code.len();

    // Mirror production order: the gate fires before Cranelift is consulted.
    if words > SIZE_GATE_WORDS {
        out.push(TierRow {
            name,
            words,
            tier: Tier::Gate(format!("too large (>{SIZE_GATE_WORDS} words)")),
            frame_aware: false,
            fa_reasons: Vec::new(),
        });
    } else {
        let constants = constants_for_inspect(proto);
        let insp = inspect(proto, &constants, helpers, isa, &NoLinker);
        let tier = match &insp.route {
            Ok(()) => Tier::Clif,
            Err(e) => Tier::Bail(e.clone()),
        };
        out.push(TierRow {
            name,
            words,
            tier,
            frame_aware: insp.frame_aware,
            fa_reasons: insp.fa_reasons,
        });
    }

    for entry in &proto.chunk.constants {
        if let PoolEntry::Function(f) = entry {
            walk(f, helpers, isa, out);
        }
    }
}

/// Heap-free constant resolution: only `is_int()` fidelity affects the
/// lowering's kind classification, so scalars map exactly and heap literals
/// become non-int placeholders.
pub(crate) fn constants_for_inspect(proto: &FunctionProto) -> Vec<VmValue> {
    proto
        .chunk
        .constants
        .iter()
        .map(|entry| match entry {
            PoolEntry::Literal(Literal::Null) => VmValue::null(),
            PoolEntry::Literal(Literal::Bool(b)) => VmValue::from_bool(*b),
            PoolEntry::Literal(Literal::Int(n)) => VmValue::from_int(*n),
            PoolEntry::Literal(Literal::Float(f)) => VmValue::from_f64(*f),
            _ => VmValue::null(),
        })
        .collect()
}

fn matches_filter(row: &TierRow, flags: &DebugFlags) -> bool {
    match &flags.fn_filter {
        None => true,
        Some(needle) => row.name.contains(needle.as_str()),
    }
}

/// `header` labels the module and is printed only when there is something to
/// show, so a `--fn` filter does not leave a trail of empty module banners.
pub fn debug_tiers(
    proto: &FunctionProto,
    flags: &DebugFlags,
    helpers: &JitHelpers,
    header: Option<&str>,
) {
    let rows: Vec<TierRow> = classify(proto, helpers)
        .into_iter()
        .filter(|r| matches_filter(r, flags))
        .collect();
    if rows.is_empty() {
        return;
    }

    let routed = rows.iter().filter(|r| r.tier == Tier::Clif).count();
    let name_w = rows
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(8)
        .clamp(8, 32);

    if let Some(h) = header {
        eprintln!("\n{DIM}=== {h} ==={R}");
    }
    eprintln!(
        "\n{BOLD}TIERS{R}{DIM} ── {} · {routed}/{} ruteadas{R}",
        proto.name.as_deref().unwrap_or("<module>"),
        rows.len()
    );
    eprintln!(
        "  {DIM}{:<name_w$}  {:>6}  {:<5}  razón{R}",
        "función", "words", "tier"
    );
    for r in &rows {
        let fa = if r.frame_aware {
            format!(" (frame-aware: {})", r.fa_reasons.join("+"))
        } else {
            String::new()
        };
        eprintln!(
            "  {:<name_w$}  {:>6}  {}{:<5}{R}  {DIM}{}{fa}{R}",
            truncate(&r.name, name_w),
            r.words,
            r.colour(),
            r.marker(),
            r.reason(),
        );
    }
}

/// Only prints when something is blocked. A clean module producing no output
/// is what lets `-p bails` over a whole program read as a punch list.
pub fn debug_bails(
    proto: &FunctionProto,
    flags: &DebugFlags,
    helpers: &JitHelpers,
    header: Option<&str>,
) {
    let rows: Vec<TierRow> = classify(proto, helpers)
        .into_iter()
        .filter(|r| r.tier != Tier::Clif && matches_filter(r, flags))
        .collect();
    if rows.is_empty() {
        return;
    }

    if let Some(h) = header {
        eprintln!("\n{DIM}=== {h} ==={R}");
    }
    eprintln!(
        "\n{BOLD}BAILS{R}{DIM} ── {}{R}",
        proto.name.as_deref().unwrap_or("<module>")
    );

    for (kind, colour) in [("gate", YELLOW), ("lowering", RED)] {
        let group: Vec<&TierRow> = rows
            .iter()
            .filter(|r| match r.tier {
                Tier::Gate(_) => kind == "gate",
                Tier::Bail(_) => kind == "lowering",
                Tier::Clif => false,
            })
            .collect();
        if group.is_empty() {
            continue;
        }
        eprintln!("  {colour}{kind}{R} {DIM}({}){R}", group.len());
        for r in group {
            eprintln!(
                "    {:<32} {:>6} words   {DIM}{}{R}",
                truncate(&r.name, 32),
                r.words,
                r.reason()
            );
        }
    }
}

fn truncate(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_owned();
    }
    let keep = width.saturating_sub(1);
    let head: String = s.chars().take(keep).collect();
    format!("{head}…")
}
