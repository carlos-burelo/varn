use std::time::Duration;

use varn_core::OpCode;
use varn_utilities::chalk::chalk;
use varn_utilities::terminal;
use varn_vm::varn_jit::JitStatsSnapshot;
use varn_vm::VmProfile;

pub fn print_parse_breakdown(profile: &varn_parser::ParseProfile) {
    let rows = [
        ("program_loop", profile.program_loop),
        ("stmt_or_decl", profile.stmt_or_decl),
        ("block", profile.block),
        ("recover", profile.recover),
    ];
    print_duration_rows_green("Parser Breakdown", &rows);
}

pub fn print_check_breakdown(profile: &varn_checker::CheckProfile) {
    let rows = [
        ("load_globals", profile.load_globals),
        ("bind", profile.bind),
        ("merge_core", profile.merge_core_members),
        ("enrich_calls", profile.enrich_call_returns),
        ("check_stmts", profile.check_stmts),
        ("annotations", profile.collect_annotations),
        ("finalize", profile.finalize),
    ];
    print_duration_rows_red("Checker Breakdown", &rows);
}

pub fn print_vm_profile(profile: &VmProfile) {
    terminal::blank();
    terminal::log(chalk("VM Profile").cyan().bold());

    let ic_total = profile.ic_hits + profile.ic_misses;
    let ic_hit_rate = if ic_total > 0 {
        profile.ic_hits as f64 / ic_total as f64 * 100.0
    } else {
        0.0
    };
    terminal::log(format!(
        "  {:<22} {:>10}  ({:.1}% hit rate)",
        "IC hits",
        fmt_num_u64(profile.ic_hits),
        ic_hit_rate
    ));
    terminal::log(format!(
        "  {:<22} {:>10}",
        "IC misses",
        fmt_num_u64(profile.ic_misses)
    ));

    if profile.ic_hits_getprop > 0 || profile.ic_misses_getprop > 0 {
        let getprop_total = profile.ic_hits_getprop + profile.ic_misses_getprop;
        let getprop_rate = if getprop_total > 0 {
            profile.ic_hits_getprop as f64 / getprop_total as f64 * 100.0
        } else {
            0.0
        };
        terminal::log(format!(
            "  {:<22} {:>10}  ({:.1}% hit rate)",
            "  GetProp IC hits",
            fmt_num_u64(profile.ic_hits_getprop),
            getprop_rate
        ));
    }

    if profile.ic_hits_setprop > 0 || profile.ic_misses_setprop > 0 {
        let setprop_total = profile.ic_hits_setprop + profile.ic_misses_setprop;
        let setprop_rate = if setprop_total > 0 {
            profile.ic_hits_setprop as f64 / setprop_total as f64 * 100.0
        } else {
            0.0
        };
        terminal::log(format!(
            "  {:<22} {:>10}  ({:.1}% hit rate)",
            "  SetProp IC hits",
            fmt_num_u64(profile.ic_hits_setprop),
            setprop_rate
        ));
    }

    if profile.ic_hits_callmethod > 0 || profile.ic_misses_callmethod > 0 {
        let cm_total = profile.ic_hits_callmethod + profile.ic_misses_callmethod;
        let cm_rate = if cm_total > 0 {
            profile.ic_hits_callmethod as f64 / cm_total as f64 * 100.0
        } else {
            0.0
        };
        terminal::log(format!(
            "  {:<22} {:>10}  ({:.1}% hit rate)",
            "  CallMethod IC hits",
            fmt_num_u64(profile.ic_hits_callmethod),
            cm_rate
        ));
    }

    let call_total = profile.calls_vm_fast + profile.calls_prepare_slow + profile.calls_native;
    let pct = |n: u64| -> f64 {
        if call_total > 0 {
            n as f64 / call_total as f64 * 100.0
        } else {
            0.0
        }
    };
    terminal::log(format!(
        "  {:<22} {:>10}  ({:.1}%)",
        "calls vm-fast",
        fmt_num_u64(profile.calls_vm_fast),
        pct(profile.calls_vm_fast)
    ));
    terminal::log(format!(
        "  {:<22} {:>10}  ({:.1}%)",
        "calls slow/prepare",
        fmt_num_u64(profile.calls_prepare_slow),
        pct(profile.calls_prepare_slow)
    ));
    terminal::log(format!(
        "  {:<22} {:>10}  ({:.1}%)",
        "calls native",
        fmt_num_u64(profile.calls_native),
        pct(profile.calls_native)
    ));
    terminal::log(format!(
        "  {:<22} {:>10}",
        "heap allocs",
        fmt_num_u64(profile.heap_allocs)
    ));
    terminal::blank();
    terminal::log(chalk("GC Stats").cyan().bold());
    terminal::log(format!(
        "  {:<22} {:>10}",
        "nursery allocs",
        fmt_num_u64(profile.nursery_allocs)
    ));
    terminal::log(format!(
        "  {:<22} {:>10}",
        "minor gc runs",
        fmt_num_u64(profile.minor_gc_count)
    ));
    terminal::log(format!(
        "  {:<22} {:>10}",
        "minor gc promoted",
        fmt_num_u64(profile.minor_gc_promoted)
    ));
    terminal::log(format!(
        "  {:<22} {:>10}",
        "gc collections",
        fmt_num_u64(profile.gc_collections)
    ));
    terminal::log(format!("  {:<22} {:>10}", "gc freed", fmt_num_u64(profile.gc_freed)));
    terminal::log(format!(
        "  {:<22} {:>10}",
        "heap live (post-gc)",
        fmt_num_u64(profile.heap_live)
    ));
    terminal::log(format!(
        "  {:<22} {:>10}",
        "heap total slots",
        fmt_num_u64(profile.heap_total)
    ));
    terminal::blank();
    terminal::log(chalk("Register VM Stats").cyan().bold());
    terminal::log(format!(
        "  {:<22} {:>10}",
        "Move opcodes",
        fmt_num_u64(profile.move_opcodes)
    ));
    terminal::log(format!(
        "  {:<22} {:>10}",
        "frame pushes",
        fmt_num_u64(profile.frame_pushes)
    ));
    terminal::log(format!(
        "  {:<22} {:>10}",
        "frame pops",
        fmt_num_u64(profile.frame_pops)
    ));
    terminal::blank();
}

pub fn print_opcode_hotspots(rows: &[(OpCode, u64)]) {
    let total: u64 = rows.iter().map(|(_, count)| *count).sum();
    if total == 0 {
        return;
    }

    terminal::blank();
    terminal::log(chalk("VM Opcode Hotspots").blue().bold());
    for (op, count) in rows.iter().take(12) {
        let share = *count as f64 / total as f64;
        terminal::log(format!(
            "  {:<20} {:>10}  {:>4.0}%",
            format!("{op:?}").trim_start_matches("Op"),
            fmt_num_u64(*count),
            share * 100.0
        ));
    }
    terminal::log(format!("  {:<20} {:>10}", "total", fmt_num_u64(total)));
    terminal::blank();
}

fn print_duration_rows_green(title: &str, rows: &[(&str, Duration)]) {
    let total: Duration = rows.iter().map(|(_, d)| *d).sum();
    if total.is_zero() {
        return;
    }

    terminal::blank();
    terminal::log(chalk(title).green().bold());
    for (name, dur) in rows {
        let share = dur.as_nanos() as f64 / total.as_nanos() as f64;
        terminal::log(format!(
            "  {:<14} {:>10}  {:>4.0}%",
            name,
            fmt_dur(*dur),
            share * 100.0
        ));
    }
    terminal::log(format!("  {:<14} {:>10}", "total", fmt_dur(total)));
    terminal::blank();
}

fn print_duration_rows_red(title: &str, rows: &[(&str, Duration)]) {
    let total: Duration = rows.iter().map(|(_, d)| *d).sum();
    if total.is_zero() {
        return;
    }

    terminal::blank();
    terminal::log(chalk(title).red().bold());
    for (name, dur) in rows {
        let share = dur.as_nanos() as f64 / total.as_nanos() as f64;
        terminal::log(format!(
            "  {:<14} {:>10}  {:>4.0}%",
            name,
            fmt_dur(*dur),
            share * 100.0
        ));
    }
    terminal::log(format!("  {:<14} {:>10}", "total", fmt_dur(total)));
    terminal::blank();
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

pub fn print_jit_stats(stats: &JitStatsSnapshot) {
    terminal::blank();
    terminal::log(chalk("JIT Compiler & Execution Stats").magenta().bold());

    let total_compilations = stats.compile_success + stats.compile_fail;
    terminal::log(format!(
        "  {:<26} {:>10}  (success: {}, failed: {})",
        "freshly compiled",
        fmt_num_u64(total_compilations),
        stats.compile_success,
        stats.compile_fail
    ));
    terminal::log(format!(
        "  {:<26} {:>10}",
        "using cached JIT",
        fmt_num_u64(stats.jit_cached)
    ));

    let compile_time = Duration::from_nanos(stats.total_compile_time_ns);
    terminal::log(format!(
        "  {:<26} {:>10}",
        "total compile time",
        fmt_dur(compile_time)
    ));

    terminal::log(format!(
        "  {:<26} {:>10}",
        "total machine code",
        fmt_bytes(stats.total_code_size_bytes as usize)
    ));

    let total_runs = stats.jit_runs + stats.interp_runs;
    let jit_ratio = if total_runs > 0 {
        stats.jit_runs as f64 / total_runs as f64 * 100.0
    } else {
        0.0
    };
    let interp_ratio = if total_runs > 0 {
        stats.interp_runs as f64 / total_runs as f64 * 100.0
    } else {
        0.0
    };

    terminal::log(format!(
        "  {:<26} {:>10}  ({:.1}%)",
        "JIT runs",
        fmt_num_u64(stats.jit_runs),
        jit_ratio
    ));
    terminal::log(format!(
        "  {:<26} {:>10}  ({:.1}%)",
        "interpreted runs",
        fmt_num_u64(stats.interp_runs),
        interp_ratio
    ));
    terminal::blank();
}

fn fmt_bytes(n: usize) -> String {
    if n < 1_024 {
        format!("{n} B")
    } else if n < 1_048_576 {
        format!("{:.1} KB", n as f64 / 1_024.0)
    } else {
        format!("{:.2} MB", n as f64 / 1_048_576.0)
    }
}
