//! Cranelift coverage — the section that replaces `JIT Compiler & Execution
//! Stats`.
//!
//! The number it exists to report honestly is what fraction of the program
//! runs as compiled code. Two things make that easy to overstate, and both are
//! handled here:
//!
//! * A function turned away by the size gate never reaches Cranelift, so it
//!   appears in neither `compile_fail` nor any `CLIF BAIL` trace. `routed +
//!   gated + bailed` is printed as an explicit total so a missing category is
//!   visible rather than silent.
//! * Function counts and frame counts are different denominators. One function
//!   left interpreted can account for most frame entries, or almost none. They
//!   are reported as separate blocks, never mixed into one ratio.

use std::collections::BTreeMap;
use std::time::Duration;

use varn_term::chalk::chalk;
use varn_term::terminal;
use varn_vm::varn_jit::{CompileOutcome, CompileRecord, JitStatsSnapshot};

use super::fmt::{fmt_bytes, fmt_dur, fmt_num, fmt_pct, row, LABEL_WIDTH, VALUE_WIDTH};

const TOP_N: usize = 3;

/// Highest-impact blocker: the reason keeping the most functions out of clif.
pub fn top_blocker(records: &[CompileRecord]) -> Option<(String, String)> {
    let mut worst: Option<&CompileRecord> = None;
    for r in records {
        if r.outcome.is_routed() {
            continue;
        }
        // Prefer the largest offender; size is the proxy for how much work is
        // stranded outside clif.
        if worst.is_none_or(|w| r.words > w.words) {
            worst = Some(r);
        }
    }
    worst.map(|r| {
        let kind = match &r.outcome {
            CompileOutcome::Gated(_) => "gate",
            CompileOutcome::Bailed(_) => "bail",
            CompileOutcome::Routed => unreachable!("filtered above"),
        };
        (
            r.name.clone(),
            format!(
                "{kind}: {} [{} words]",
                r.outcome.reason().unwrap_or("desconocido"),
                r.words
            ),
        )
    })
}

pub fn print_coverage(jit: &JitStatsSnapshot, records: &[CompileRecord], scope: &str) {
    terminal::blank();
    terminal::log(format!(
        "{} {}",
        chalk("Cobertura Cranelift").magenta().bold(),
        chalk(format!("[ámbito: {scope}]")).dim()
    ));

    let routed = jit.compile_success;
    let gated = jit.gate_rejected;
    let bailed = jit.compile_fail;
    let seen = jit.functions_seen();
    let pct = |n: u64| -> String {
        if seen == 0 {
            "—".to_owned()
        } else {
            fmt_pct(n as f64 / seen as f64)
        }
    };

    let gate_note = first_reason(records, |o| matches!(o, CompileOutcome::Gated(_)));
    let bail_note = first_reason(records, |o| matches!(o, CompileOutcome::Bailed(_)));

    terminal::log(format!(
        "{}  {}",
        row("ruteadas", fmt_num(routed)),
        chalk(pct(routed)).dim()
    ));
    terminal::log(format!(
        "{}  {}  {}",
        row("gate (nunca ofrecidas)", fmt_num(gated)),
        chalk(pct(gated)).dim(),
        chalk(gate_note.unwrap_or_default()).dim()
    ));
    terminal::log(format!(
        "{}  {}  {}",
        row("bail de lowering", fmt_num(bailed)),
        chalk(pct(bailed)).dim(),
        chalk(bail_note.unwrap_or_default()).dim()
    ));
    terminal::log(format!(
        "  {}",
        chalk("─".repeat(LABEL_WIDTH + VALUE_WIDTH + 1)).dim()
    ));
    terminal::log(format!(
        "{}  {}",
        row("ofrecidas al JIT", fmt_num(seen)),
        chalk(pct(seen)).dim()
    ));
    terminal::log(format!(
        "  {}",
        chalk(
            "denominador = funciones que el JIT llegó a ver, incluido el top level \
             del módulo cuando OSR lo rescata; los generadores no entran"
        )
        .dim()
    ));

    terminal::blank();
    let frame_pct = |n: u64| -> String {
        if jit.total_frames() == 0 {
            "—".to_owned()
        } else {
            fmt_pct(jit.frame_share_of(n))
        }
    };
    terminal::log(format!(
        "{}  {}",
        row("frames clif", fmt_num(jit.jit_runs)),
        chalk(frame_pct(jit.jit_runs)).dim()
    ));
    // An OSR frame is one of the interpreter frames: it STARTED interpreted and
    // was rescued mid-loop. Reporting the two as one number said that a
    // top-level loop runs interpreted when it does not — measured on a 3M
    // iteration loop at module top level, 88.87 ms with `VARN_NO_JIT=1` against
    // 7.78 ms with OSR, and the frame counted as "interpreter" either way.
    // Split them, so the line that reads as a defect only counts frames that
    // really did run to the end on the interpreter.
    let never_compiled = jit.never_compiled_frames();
    terminal::log(format!(
        "{}  {}",
        row("frames intérprete", fmt_num(jit.interp_runs)),
        chalk(frame_pct(jit.interp_runs)).dim()
    ));
    if jit.osr_entries > 0 {
        terminal::log(format!(
            "{}  {}",
            row("  rescatados por OSR", fmt_num(jit.osr_entries)),
            chalk("← empezaron interpretados, terminaron compilados").dim()
        ));
    }
    let never_line = format!(
        "{}  {}",
        row("  nunca compilados", fmt_num(never_compiled)),
        frame_pct(never_compiled)
    );
    if never_compiled > 0 {
        terminal::log(format!(
            "{}  {}",
            chalk(never_line).yellow(),
            chalk("← frías por tiering, o formas que el JIT no toma").dim()
        ));
    } else {
        terminal::log(chalk(never_line).green().to_string());
    }
    terminal::log(format!(
        "{}  {}",
        row("de esos, cache hit", fmt_num(jit.jit_cached)),
        chalk(frame_pct(jit.jit_cached)).dim()
    ));

    terminal::blank();
    let compile = Duration::from_nanos(jit.total_compile_time_ns);
    let per_fn = jit
        .ns_per_routed_fn()
        .map(|ns| fmt_dur(Duration::from_nanos(ns as u64)))
        .unwrap_or_else(|| "—".to_owned());
    terminal::log(format!(
        "  {}   {}   {}   {}",
        chalk("compilar").yellow().bold(),
        fmt_dur(compile),
        chalk(format!("{per_fn}/fn")).dim(),
        chalk(fmt_bytes(jit.total_code_size_bytes)).dim()
    ));
    if jit.backend_time_ns > 0 {
        let backend = Duration::from_nanos(jit.backend_time_ns);
        let lowering = compile.saturating_sub(backend);
        let pct = jit.backend_time_ns as f64 / jit.total_compile_time_ns.max(1) as f64 * 100.0;
        terminal::log(format!(
            "  {}",
            chalk(format!(
                "de eso:  cranelift {} ({pct:.0}%)  ·  lowering {}",
                fmt_dur(backend),
                fmt_dur(lowering)
            ))
            .dim()
        ));
    }

    let mut by_cost: Vec<&CompileRecord> = records.iter().filter(|r| r.compile_ns > 0).collect();
    by_cost.sort_by(|a, b| b.compile_ns.cmp(&a.compile_ns));
    if !by_cost.is_empty() {
        let top = by_cost
            .iter()
            .take(TOP_N)
            .map(|r| format!("{} {}", r.name, fmt_dur(Duration::from_nanos(r.compile_ns))))
            .collect::<Vec<_>>()
            .join(" · ");
        terminal::log(format!("  {}", chalk(format!("top-{TOP_N}:  {top}")).dim()));
    }

    if !records.is_empty() {
        print_blockers(records);
    }
}

/// Group everything not routed by reason, so the output names causes rather
/// than listing symptoms one function at a time.
fn print_blockers(records: &[CompileRecord]) {
    let mut groups: BTreeMap<String, Vec<&CompileRecord>> = BTreeMap::new();
    for r in records {
        if let Some(reason) = r.outcome.reason() {
            let kind = match r.outcome {
                CompileOutcome::Gated(_) => "gate",
                _ => "bail",
            };
            groups
                .entry(format!("{kind}: {reason}"))
                .or_default()
                .push(r);
        }
    }
    if groups.is_empty() {
        return;
    }

    terminal::blank();
    terminal::log(format!("  {}", chalk("fuera de clif").red().bold()));
    for (reason, fns) in groups {
        terminal::log(format!(
            "    {}  {}",
            chalk(&reason).red(),
            chalk(format!("({})", fns.len())).dim()
        ));
        for r in fns.iter().take(TOP_N) {
            terminal::log(format!(
                "      {:<24} {:>6} words",
                r.name,
                fmt_num(r.words as u64)
            ));
        }
        if fns.len() > TOP_N {
            terminal::log(format!(
                "      {}",
                chalk(format!("… y {} más", fns.len() - TOP_N)).dim()
            ));
        }
    }
}

fn first_reason(
    records: &[CompileRecord],
    pred: impl Fn(&CompileOutcome) -> bool,
) -> Option<String> {
    records
        .iter()
        .find(|r| pred(&r.outcome))
        .and_then(|r| r.outcome.reason().map(|s| s.to_owned()))
}
