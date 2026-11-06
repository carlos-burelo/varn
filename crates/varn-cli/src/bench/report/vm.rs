use std::time::Duration;

use varn_core::OpCode;
use varn_vm::VmProfile;

use super::fmt::{percent, share, DurScale};
use super::plain::{
    Align::{Left, Right},
    Report, Table,
};

pub fn breakdown(
    report: &mut Report,
    title: &str,
    rows: &[(&str, Duration)],
    measured: Option<Duration>,
) {
    let total: Duration = rows.iter().map(|(_, d)| *d).sum();
    if total.is_zero() {
        return;
    }
    let scale = DurScale::for_column(rows.iter().map(|(_, d)| *d).chain([total]).chain(measured));
    report.title(title);
    let mut table = Table::new(
        ["part", &scale.header("time"), "share%"],
        [Left, Right, Right],
    );
    let nanos = |d: Duration| d.as_nanos() as u64;
    for (name, duration) in rows {
        table.row([
            (*name).to_owned(),
            scale.num(*duration),
            share(nanos(*duration), nanos(total)),
        ]);
    }
    table.row(["subtotal".to_owned(), scale.num(total), "100.0".to_owned()]);
    if let Some(measured) = measured {
        let residual = measured.saturating_sub(total);
        table.row([
            "unattributed".to_owned(),
            scale.num(residual),
            share(nanos(residual), nanos(measured)),
        ]);
        table.row(["measured".to_owned(), scale.num(measured), "-".to_owned()]);
    }
    report.table(&table);
}

pub fn opcodes(report: &mut Report, rows: &[(OpCode, u64)], interp_share: Option<f64>) {
    let total: u64 = rows.iter().map(|(_, count)| *count).sum();
    if total == 0 {
        return;
    }
    report.title(&match interp_share {
        Some(ratio) => format!("opcodes (interpreter frames: {}%)", percent(ratio)),
        None => "opcodes".to_owned(),
    });
    let mut table = Table::new(["opcode", "count", "share%"], [Left, Right, Right]);
    for (op, count) in rows.iter().take(TOP_OPCODES) {
        table.row([format!("{op:?}"), count.to_string(), share(*count, total)]);
    }
    table.row(["total".to_owned(), total.to_string(), "100.0".to_owned()]);
    report.table(&table);
}

const TOP_OPCODES: usize = 16;

pub fn counters(report: &mut Report, profile: &VmProfile) {
    let rate = |hits: u64, misses: u64| share(hits, hits + misses);

    report.title("inline caches");
    let mut table = Table::new(
        ["site", "hits", "misses", "hit%"],
        [Left, Right, Right, Right],
    );
    table.row([
        "total".to_owned(),
        profile.ic_hits.to_string(),
        profile.ic_misses.to_string(),
        rate(profile.ic_hits, profile.ic_misses),
    ]);
    for (name, hits, misses) in [
        (
            "getprop",
            profile.ic_hits_getprop,
            profile.ic_misses_getprop,
        ),
        (
            "setprop",
            profile.ic_hits_setprop,
            profile.ic_misses_setprop,
        ),
        (
            "callmethod",
            profile.ic_hits_callmethod,
            profile.ic_misses_callmethod,
        ),
    ] {
        if hits + misses > 0 {
            table.row([
                name.to_owned(),
                hits.to_string(),
                misses.to_string(),
                rate(hits, misses),
            ]);
        }
    }
    report.table(&table);

    let calls = profile.calls_vm_fast + profile.calls_prepare_slow + profile.calls_native;
    report.title("calls");
    let mut table = Table::new(["kind", "count", "share%"], [Left, Right, Right]);
    for (name, count) in [
        ("vm-fast", profile.calls_vm_fast),
        ("slow", profile.calls_prepare_slow),
        ("native", profile.calls_native),
    ] {
        table.row([name.to_owned(), count.to_string(), share(count, calls)]);
    }
    table.row(["total".to_owned(), calls.to_string(), share(calls, calls)]);
    report.table(&table);

    report.title("gc");
    let mut table = Table::pairs();
    for (name, value) in [
        ("major pause total ns", profile.gc_ns),
        ("major pause max ns", profile.gc_max_pause_ns),
        ("minor pause total ns", profile.minor_gc_ns),
        ("minor pause max ns", profile.minor_gc_max_pause_ns),
    ] {
        table.row([name.to_owned(), value.to_string()]);
    }
    for (name, value) in [
        ("young allocs", profile.nursery_allocs),
        ("heap allocs", profile.heap_allocs),
        ("minor runs", profile.minor_gc_count),
        ("minor promoted", profile.minor_gc_promoted),
        ("collections", profile.gc_collections),
        ("freed", profile.gc_freed),
        ("heap live", profile.heap_live),
        ("heap slots", profile.heap_total),
    ] {
        table.row([name.to_owned(), value.to_string()]);
    }
    report.table(&table);

    report.title("frames");
    let mut table = Table::pairs();
    for (name, value) in [
        ("move opcodes", profile.move_opcodes),
        ("pushes", profile.frame_pushes),
        ("pops", profile.frame_pops),
    ] {
        table.row([name.to_owned(), value.to_string()]);
    }
    report.table(&table);

    report.title("tasks");
    let mut table = Table::pairs();
    for (name, value) in [
        ("parks", profile.task_parks),
        ("released", profile.task_released),
        ("prune hit", profile.task_prune_hit),
        ("prune miss", profile.task_prune_miss),
        ("yields", profile.task_yields),
        ("timers purged", profile.timer_purged),
    ] {
        table.row([name.to_owned(), value.to_string()]);
    }
    report.table(&table);
}
