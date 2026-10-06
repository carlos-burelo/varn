









use varn_jit::clif::debug::inspect;
use varn_jit::clif::lower::{gate_reason, NoLinker};
use varn_jit::JitHelpers;
use varn_types::{FunctionProto, PoolEntry};

use crate::render::truncate;
use crate::walk::constants_for_inspect;

use crate::flags::DebugFlags;

use varn_core::term::terminal;
use varn_core::term::terminal::{Align, Section};


#[derive(PartialEq, Eq)]
pub enum Tier {
    Clif,
    
    Gate(String),
    
    Bail(String),
}

pub struct TierRow {
    pub name: String,
    pub words: usize,
    pub tier: Tier,
    pub frame_aware: bool,
    pub framed: bool,
    
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

    fn reason(&self) -> &str {
        match &self.tier {
            Tier::Clif => "",
            Tier::Gate(r) | Tier::Bail(r) => r,
        }
    }
}


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

    
    if let Some(reason) = gate_reason(proto) {
        out.push(TierRow {
            name,
            words,
            tier: Tier::Gate(reason),
            frame_aware: false,
            framed: false,
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
            framed: insp.framed,
            fa_reasons: insp.fa_reasons,
        });
    }

    for entry in &proto.chunk.constants {
        if let PoolEntry::Function(f) = entry {
            walk(f, helpers, isa, out);
        }
    }
}

fn matches_filter(row: &TierRow, flags: &DebugFlags) -> bool {
    match &flags.fn_filter {
        None => true,
        Some(needle) => row.name.contains(needle.as_str()),
    }
}



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
        terminal::tagged("module", h);
    }
    Section::new("tiers")
        .subtitle(format!(
            "{} · {routed}/{} ruteadas",
            proto.name.as_deref().unwrap_or("<module>"),
            rows.len()
        ))
        .color(|c| c.bold())
        .print();
    let mut table = terminal::Table::new(["función", "words", "tier", "razón"]).align([
        Align::Left,
        Align::Right,
        Align::Left,
        Align::Left,
    ]);
    for r in &rows {
        let mut detail = r.reason().to_string();
        if r.frame_aware {
            let abi = if r.framed { "framed" } else { "native" };
            detail.push_str(&format!(" ({abi}: {})", r.fa_reasons.join("+")));
        }
        table.row([
            truncate(&r.name, name_w).to_string(),
            r.words.to_string(),
            r.marker().to_string(),
            detail,
        ]);
    }
    table.print();
    Section::new("tiers").close();
}



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
        terminal::tagged("module", h);
    }
    Section::new("bails")
        .subtitle(proto.name.as_deref().unwrap_or("<module>"))
        .color(|c| c.bold())
        .print();

    for kind in ["gate", "lowering"] {
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
        terminal::tagged(kind, format!("{} bloqueada(s)", group.len()));
        let mut table = terminal::Table::new(["función", "words", "razón"]).align([
            Align::Left,
            Align::Right,
            Align::Left,
        ]);
        for r in group {
            table.row([
                truncate(&r.name, 32).to_string(),
                format!("{} words", r.words),
                r.reason().to_string(),
            ]);
        }
        table.print();
    }
    Section::new("bails").close();
}
