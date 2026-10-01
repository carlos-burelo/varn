//! `vn debug -p summary` — one page describing what was compiled.
//!
//! Exists because the other phases are all firehoses: `-p bytecode` on a real
//! module prints everything and answers nothing about proportion. This answers
//! "how big is it and where is the weight" first, so you know which function to
//! then dump.

use varn_types::{FunctionProto, PoolEntry};

use crate::render::truncate;

use varn_core::term::terminal;
use varn_core::term::terminal::{Align, Section};

const TOP_N: usize = 10;

struct FnSize {
    name: String,
    words: usize,
    constants: usize,
    gated: bool,
}

fn collect(proto: &FunctionProto, out: &mut Vec<FnSize>) {
    out.push(FnSize {
        name: proto.name.as_deref().unwrap_or("<module>").to_owned(),
        words: proto.chunk.code.len(),
        constants: proto.chunk.constants.len(),
        gated: varn_jit::clif::lower::gate_reason(proto).is_some(),
    });
    for entry in &proto.chunk.constants {
        if let PoolEntry::Function(f) = entry {
            collect(f, out);
        }
    }
}

pub fn debug_summary(proto: &FunctionProto) {
    let mut fns = Vec::new();
    collect(proto, &mut fns);

    let total_words: usize = fns.iter().map(|f| f.words).sum();
    let total_consts: usize = fns.iter().map(|f| f.constants).sum();
    let over_gate = fns.iter().filter(|f| f.gated).count();

    Section::new("summary")
        .subtitle(proto.name.as_deref().unwrap_or("<module>"))
        .color(|c| c.bold())
        .print();
    let mut stats =
        terminal::Table::new(["métrica", "valor"]).align([Align::Left, Align::Right]);
    stats.row(["funciones".to_string(), fns.len().to_string()]);
    stats.row(["bytecode (words)".to_string(), total_words.to_string()]);
    stats.row(["constantes".to_string(), total_consts.to_string()]);
    stats.row(["exports".to_string(), proto.export_names.len().to_string()]);
    stats.row([
        "fuera de clif por tamaño".to_string(),
        format!("{over_gate} (gate {} words)", varn_jit::SIZE_GATE_WORDS),
    ]);
    stats.print();

    fns.sort_by_key(|b| std::cmp::Reverse(b.words));
    terminal::tagged("top", format!("{TOP_N} por tamaño"));
    let mut top =
        terminal::Table::new(["función", "words", "gate"]).align([Align::Left, Align::Right, Align::Left]);
    for f in fns.iter().take(TOP_N) {
        top.row([
            truncate(&f.name, 32).to_string(),
            format!("{} words", f.words),
            if f.gated { "excede el gate".to_string() } else { String::new() },
        ]);
    }
    top.print();
    Section::new("summary").close();
}
