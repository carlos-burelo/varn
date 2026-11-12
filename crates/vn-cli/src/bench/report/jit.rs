use std::collections::BTreeMap;
use std::time::Duration;

use vn_jit::{CompileOutcome, CompileRecord, JitStatsSnapshot};

use super::fmt::{fmt_bytes, fmt_dur, share};
use super::plain::{
    Align::{Left, Right},
    Report, Table,
};

const TOP_COST: usize = 5;
const TOP_BLOCKED: usize = 5;

pub fn top_blocker(records: &[CompileRecord]) -> Option<(String, String)> {
    let mut worst: Option<&CompileRecord> = None;
    for r in records {
        if r.outcome.is_routed() {
            continue;
        }

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
                r.outcome.reason().unwrap_or("unknown"),
                r.words
            ),
        )
    })
}

pub fn coverage(
    report: &mut Report,
    jit: &JitStatsSnapshot,
    records: &[CompileRecord],
    scope: &str,
) {
    let seen = jit.functions_seen();
    report.title(&format!("jit functions ({scope})"));
    let mut table = Table::new(
        ["outcome", "count", "share%", "reason"],
        [Left, Right, Right, Left],
    );
    let reason = |wanted: fn(&CompileOutcome) -> bool| -> String {
        records
            .iter()
            .find(|r| wanted(&r.outcome))
            .and_then(|r| r.outcome.reason())
            .unwrap_or("")
            .to_owned()
    };
    table.row([
        "routed".to_owned(),
        jit.compile_success.to_string(),
        share(jit.compile_success, seen),
        String::new(),
    ]);
    table.row([
        "gated".to_owned(),
        jit.gate_rejected.to_string(),
        share(jit.gate_rejected, seen),
        reason(|o| matches!(o, CompileOutcome::Gated(_))),
    ]);
    table.row([
        "bailed".to_owned(),
        jit.compile_fail.to_string(),
        share(jit.compile_fail, seen),
        reason(|o| matches!(o, CompileOutcome::Bailed(_))),
    ]);
    table.row([
        "seen".to_owned(),
        seen.to_string(),
        share(seen, seen),
        String::new(),
    ]);
    report.table(&table);

    let frames = jit.total_frames();
    report.title("jit frames");
    let mut table = Table::new(["kind", "count", "share%"], [Left, Right, Right]);
    for (name, count) in [
        ("clif", jit.jit_runs),
        ("interpreter", jit.interp_runs),
        ("interpreter never compiled", jit.never_compiled_frames()),
        ("clif cache hit", jit.jit_cached),
        ("osr entries", jit.osr_entries),
        ("catch in native", jit.catch_entries),
    ] {
        table.row([name.to_owned(), count.to_string(), share(count, frames)]);
    }
    report.table(&table);

    let compile = Duration::from_nanos(jit.total_compile_time_ns);
    let backend = Duration::from_nanos(jit.backend_time_ns);
    report.title("jit compile");
    let mut table = Table::pairs();
    table.row(["total".to_owned(), fmt_dur(compile)]);
    table.row([
        "per function".to_owned(),
        jit.ns_per_routed_fn()
            .map(|ns| fmt_dur(Duration::from_nanos(ns as u64)))
            .unwrap_or_else(|| "-".to_owned()),
    ]);
    table.row(["code size".to_owned(), fmt_bytes(jit.total_code_size_bytes)]);
    if jit.backend_time_ns > 0 {
        table.row(["cranelift".to_owned(), fmt_dur(backend)]);
        table.row([
            "lowering".to_owned(),
            fmt_dur(compile.saturating_sub(backend)),
        ]);
    }
    report.table(&table);

    let mut by_cost: Vec<&CompileRecord> = records.iter().filter(|r| r.compile_ns > 0).collect();
    by_cost.sort_by_key(|r| std::cmp::Reverse(r.compile_ns));
    if !by_cost.is_empty() {
        report.title("jit compile cost");
        let mut table = Table::new(["function", "time", "words"], [Left, Right, Right]);
        for record in by_cost.iter().take(TOP_COST) {
            table.row([
                record.name.clone(),
                fmt_dur(Duration::from_nanos(record.compile_ns)),
                record.words.to_string(),
            ]);
        }
        report.table(&table);
    }

    blockers(report, records);
}

fn blockers(report: &mut Report, records: &[CompileRecord]) {
    let mut groups: BTreeMap<String, Vec<&CompileRecord>> = BTreeMap::new();
    for record in records {
        if let Some(reason) = record.outcome.reason() {
            let kind = match record.outcome {
                CompileOutcome::Gated(_) => "gate",
                CompileOutcome::Routed | CompileOutcome::Bailed(_) => "bail",
            };
            groups
                .entry(format!("{kind}: {reason}"))
                .or_default()
                .push(record);
        }
    }
    if groups.is_empty() {
        return;
    }
    report.title("jit blocked");
    let mut table = Table::new(
        ["reason", "functions", "words", "examples"],
        [Left, Right, Right, Left],
    );
    for (reason, mut functions) in groups {
        functions.sort_by_key(|r| std::cmp::Reverse(r.words));
        let words: usize = functions.iter().map(|r| r.words).sum();
        let examples = functions
            .iter()
            .take(TOP_BLOCKED)
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        table.row([
            reason,
            functions.len().to_string(),
            words.to_string(),
            examples,
        ]);
    }
    report.table(&table);
}
