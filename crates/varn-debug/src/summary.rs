//! `vn debug -p summary` — one page describing what was compiled.
//!
//! Exists because the other phases are all firehoses: `-p bytecode` on a real
//! module prints everything and answers nothing about proportion. This answers
//! "how big is it and where is the weight" first, so you know which function to
//! then dump.

use varn_types::{FunctionProto, PoolEntry};

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const R: &str = "\x1b[0m";

const TOP_N: usize = 10;

struct FnSize {
    name: String,
    words: usize,
    constants: usize,
}

fn collect(proto: &FunctionProto, out: &mut Vec<FnSize>) {
    out.push(FnSize {
        name: proto.name.as_deref().unwrap_or("<module>").to_owned(),
        words: proto.chunk.code.len(),
        constants: proto.chunk.constants.len(),
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
    let over_gate = fns
        .iter()
        .filter(|f| f.words > varn_jit::SIZE_GATE_WORDS)
        .count();

    eprintln!(
        "\n{BOLD}SUMMARY{R}{DIM} ── {}{R}",
        proto.name.as_deref().unwrap_or("<module>")
    );
    eprintln!("  {:<24} {:>8}", "funciones", fns.len());
    eprintln!("  {:<24} {:>8}", "bytecode (words)", total_words);
    eprintln!("  {:<24} {:>8}", "constantes", total_consts);
    eprintln!("  {:<24} {:>8}", "exports", proto.export_names.len());
    eprintln!(
        "  {:<24} {:>8}   {DIM}sobre el gate de {} words{R}",
        "fuera de clif por tamaño", over_gate, varn_jit::SIZE_GATE_WORDS
    );

    fns.sort_by(|a, b| b.words.cmp(&a.words));
    eprintln!("\n  {DIM}top-{TOP_N} por tamaño{R}");
    for f in fns.iter().take(TOP_N) {
        let flag = if f.words > varn_jit::SIZE_GATE_WORDS {
            "  ← excede el gate"
        } else {
            ""
        };
        eprintln!("    {:<32} {:>6} words{}", truncate(&f.name, 32), f.words, flag);
    }
}

fn truncate(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_owned();
    }
    let head: String = s.chars().take(width.saturating_sub(1)).collect();
    format!("{head}…")
}
