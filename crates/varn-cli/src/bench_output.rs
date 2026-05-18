use std::time::Duration;

use varn_core::OpCode;
use varn_vm::VmProfile;

const R: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const BLU: &str = "\x1b[34m";
const RED: &str = "\x1b[31m";
const GRN: &str = "\x1b[32m";
const CYN: &str = "\x1b[96m";

pub fn print_parse_breakdown(profile: &varn_parser::ParseProfile) {
    let rows = [
        ("program_loop", profile.program_loop),
        ("stmt_or_decl", profile.stmt_or_decl),
        ("block", profile.block),
        ("recover", profile.recover),
    ];
    print_duration_rows("Parser Breakdown", GRN, &rows);
}

pub fn print_check_breakdown(profile: &varn_checker::CheckProfile) {
    let rows = [
        ("load_globals", profile.load_globals),
        ("bind", profile.bind),
        ("merge_builtins", profile.merge_builtin_members),
        ("enrich_calls", profile.enrich_call_returns),
        ("check_stmts", profile.check_stmts),
        ("annotations", profile.collect_annotations),
        ("finalize", profile.finalize),
    ];
    print_duration_rows("Checker Breakdown", RED, &rows);
}

pub fn print_vm_profile(profile: &VmProfile) {
    eprintln!();
    eprintln!("  {BOLD}{CYN}VM Profile{R}{DIM}");

    let ic_total = profile.ic_hits + profile.ic_misses;
    let ic_hit_rate = if ic_total > 0 {
        profile.ic_hits as f64 / ic_total as f64 * 100.0
    } else {
        0.0
    };
    eprintln!(
        "  {:<22} {:>10}  ({:.1}% hit rate)",
        "IC hits",
        fmt_num_u64(profile.ic_hits),
        ic_hit_rate
    );
    eprintln!(
        "  {:<22} {:>10}",
        "IC misses",
        fmt_num_u64(profile.ic_misses)
    );

    if profile.ic_hits_getprop > 0 || profile.ic_misses_getprop > 0 {
        let getprop_total = profile.ic_hits_getprop + profile.ic_misses_getprop;
        let getprop_rate = if getprop_total > 0 {
            profile.ic_hits_getprop as f64 / getprop_total as f64 * 100.0
        } else {
            0.0
        };
        eprintln!(
            "  {:<22} {:>10}  ({:.1}% hit rate)",
            "  GetProp IC hits",
            fmt_num_u64(profile.ic_hits_getprop),
            getprop_rate
        );
    }

    if profile.ic_hits_setprop > 0 || profile.ic_misses_setprop > 0 {
        let setprop_total = profile.ic_hits_setprop + profile.ic_misses_setprop;
        let setprop_rate = if setprop_total > 0 {
            profile.ic_hits_setprop as f64 / setprop_total as f64 * 100.0
        } else {
            0.0
        };
        eprintln!(
            "  {:<22} {:>10}  ({:.1}% hit rate)",
            "  SetProp IC hits",
            fmt_num_u64(profile.ic_hits_setprop),
            setprop_rate
        );
    }

    if profile.ic_hits_callmethod > 0 || profile.ic_misses_callmethod > 0 {
        let cm_total = profile.ic_hits_callmethod + profile.ic_misses_callmethod;
        let cm_rate = if cm_total > 0 {
            profile.ic_hits_callmethod as f64 / cm_total as f64 * 100.0
        } else {
            0.0
        };
        eprintln!(
            "  {:<22} {:>10}  ({:.1}% hit rate)",
            "  CallMethod IC hits",
            fmt_num_u64(profile.ic_hits_callmethod),
            cm_rate
        );
    }

    let call_total = profile.calls_vm_fast + profile.calls_prepare_slow + profile.calls_native;
    let pct = |n: u64| -> f64 {
        if call_total > 0 {
            n as f64 / call_total as f64 * 100.0
        } else {
            0.0
        }
    };
    eprintln!(
        "  {:<22} {:>10}  ({:.1}%)",
        "calls vm-fast",
        fmt_num_u64(profile.calls_vm_fast),
        pct(profile.calls_vm_fast)
    );
    eprintln!(
        "  {:<22} {:>10}  ({:.1}%)",
        "calls slow/prepare",
        fmt_num_u64(profile.calls_prepare_slow),
        pct(profile.calls_prepare_slow)
    );
    eprintln!(
        "  {:<22} {:>10}  ({:.1}%)",
        "calls native",
        fmt_num_u64(profile.calls_native),
        pct(profile.calls_native)
    );
    eprintln!(
        "  {:<22} {:>10}",
        "heap allocs",
        fmt_num_u64(profile.heap_allocs)
    );
    eprintln!();
    eprintln!("  {BOLD}{CYN}GC Stats{R}{DIM}");
    eprintln!(
        "  {:<22} {:>10}",
        "gc collections",
        fmt_num_u64(profile.gc_collections)
    );
    eprintln!("  {:<22} {:>10}", "gc freed", fmt_num_u64(profile.gc_freed));
    eprintln!(
        "  {:<22} {:>10}",
        "heap live (post-gc)",
        fmt_num_u64(profile.heap_live)
    );
    eprintln!(
        "  {:<22} {:>10}",
        "heap total slots",
        fmt_num_u64(profile.heap_total)
    );
    eprintln!();
    eprintln!("  {BOLD}{CYN}Register VM Stats{R}{DIM}");
    eprintln!(
        "  {:<22} {:>10}",
        "reg loads",
        fmt_num_u64(profile.reg_loads)
    );
    eprintln!(
        "  {:<22} {:>10}",
        "reg stores",
        fmt_num_u64(profile.reg_stores)
    );
    eprintln!(
        "  {:<22} {:>10}",
        "frame pushes",
        fmt_num_u64(profile.frame_pushes)
    );
    eprintln!(
        "  {:<22} {:>10}",
        "frame pops",
        fmt_num_u64(profile.frame_pops)
    );
    eprintln!("{R}");
}

pub fn print_opcode_hotspots(rows: &[(OpCode, u64)]) {
    let total: u64 = rows.iter().map(|(_, count)| *count).sum();
    if total == 0 {
        return;
    }

    eprintln!();
    eprintln!("  {BOLD}{BLU}VM Opcode Hotspots{R}{DIM}");
    for (op, count) in rows.iter().take(12) {
        let share = *count as f64 / total as f64;
        eprintln!(
            "  {:<20} {:>10}  {:>4.0}%",
            format!("{op:?}").trim_start_matches("Op"),
            fmt_num_u64(*count),
            share * 100.0
        );
    }
    eprintln!("  {:<20} {:>10}", "total", fmt_num_u64(total));
    eprintln!("{R}");
}

fn print_duration_rows(title: &str, color: &str, rows: &[(&str, Duration)]) {
    let total: Duration = rows.iter().map(|(_, d)| *d).sum();
    if total.is_zero() {
        return;
    }

    eprintln!();
    eprintln!("  {BOLD}{color}{title}{R}{DIM}");
    for (name, dur) in rows {
        let share = dur.as_nanos() as f64 / total.as_nanos() as f64;
        eprintln!(
            "  {:<14} {:>10}  {:>4.0}%",
            name,
            fmt_dur(*dur),
            share * 100.0
        );
    }
    eprintln!("  {:<14} {:>10}", "total", fmt_dur(total));
    eprintln!("{R}");
}

fn fmt_dur(d: Duration) -> String {
    let ns = d.as_nanos();
    if ns < 1_000 {
        format!("{ns} ns")
    } else if ns < 1_000_000 {
        let us = ns as f64 / 1_000.0;
        if us < 10.0 {
            fmt_f(us, 2) + " µs"
        } else if us < 100.0 {
            fmt_f(us, 1) + " µs"
        } else {
            fmt_f(us, 0) + " µs"
        }
    } else if ns < 1_000_000_000 {
        let ms = ns as f64 / 1_000_000.0;
        if ms < 10.0 {
            fmt_f(ms, 3) + " ms"
        } else if ms < 100.0 {
            fmt_f(ms, 2) + " ms"
        } else {
            fmt_f(ms, 1) + " ms"
        }
    } else {
        let s = ns as f64 / 1_000_000_000.0;
        fmt_f(s, 2) + " s"
    }
}

fn fmt_f(f: f64, decimals: usize) -> String {
    if decimals == 0 {
        return format!("{f:.0}");
    }
    let s = format!("{f:.decimals$}");
    let s = s.trim_end_matches('0');
    s.trim_end_matches('.').to_owned()
}

fn fmt_num_u64(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}
