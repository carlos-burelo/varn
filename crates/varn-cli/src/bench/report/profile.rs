//! Interpreter, IC, GC and phase-breakdown sections.

use std::time::Duration;

use varn_core::OpCode;
use varn_term::chalk::chalk;
use varn_term::terminal;
use varn_vm::VmProfile;

use super::fmt::{fmt_dur, fmt_num, fmt_pct, row, row_note, DurScale};

const TOP_OPCODES: usize = 12;

pub struct BreakdownOpts {
    /// Print rows that measured exactly zero.
    pub all_rows: bool,
}

/// A phase breakdown, reconciled against the phase's own measured p50.
///
/// The components come from a separate instrumented run, so they never sum to
/// the timed phase exactly. Printing them without that residual invites the
/// reader to conclude the phase is fully accounted for — in practice ~30% of
/// `check` was unattributed and invisible.
pub fn print_breakdown(
    title: &str,
    colour: fn(varn_term::chalk::Chalk) -> varn_term::chalk::Chalk,
    rows: &[(&str, Duration)],
    measured: Option<Duration>,
    opts: &BreakdownOpts,
) {
    let total: Duration = rows.iter().map(|(_, d)| *d).sum();
    if total.is_zero() {
        return;
    }

    terminal::blank();
    terminal::log(colour(chalk(title)).bold());

    let scale = DurScale::for_column(rows.iter().map(|(_, d)| *d).chain([total]));
    let mut hidden = 0usize;

    for (name, dur) in rows {
        if dur.is_zero() && !opts.all_rows {
            hidden += 1;
            continue;
        }
        let share = dur.as_nanos() as f64 / total.as_nanos() as f64;
        terminal::log(format!(
            "{}  {}",
            row(name, scale.fmt(*dur)),
            chalk(fmt_pct(share)).dim()
        ));
    }

    if hidden > 0 {
        terminal::log(format!(
            "  {}",
            chalk(format!("({hidden} filas en cero ocultas — --all-rows)")).dim()
        ));
    }

    terminal::log(format!("{}", row("subtotal", scale.fmt(total))));

    if let Some(measured) = measured {
        let residual = measured.saturating_sub(total);
        if !residual.is_zero() {
            let share = residual.as_nanos() as f64 / measured.as_nanos() as f64;
            terminal::log(format!(
                "{}  {}",
                row("other (sin atribuir)", scale.fmt(residual)),
                chalk(fmt_pct(share)).yellow()
            ));
            terminal::log(format!(
                "{}  {}",
                row("fase medida (p50)", fmt_dur(measured)),
                chalk("100.0%").dim()
            ));
        }
    }
}

/// Opcode counts, which only the interpreter increments.
///
/// With most frames running as machine code these numbers describe a small
/// slice of the program. The header says which slice, because the bare title
/// reads as a profile of the whole run.
pub fn print_opcode_hotspots(rows: &[(OpCode, u64)], interp_frame_share: Option<f64>) {
    let total: u64 = rows.iter().map(|(_, count)| *count).sum();
    if total == 0 {
        return;
    }

    terminal::blank();
    let scope = match interp_frame_share {
        Some(share) => format!("[solo intérprete — {} de los frames]", fmt_pct(share)),
        None => "[solo intérprete]".to_owned(),
    };
    terminal::log(format!(
        "{} {}",
        chalk("VM Opcode Hotspots").blue().bold(),
        chalk(scope).dim()
    ));

    for (op, count) in rows.iter().take(TOP_OPCODES) {
        let share = *count as f64 / total as f64;
        terminal::log(format!(
            "{}  {}",
            row(format!("{op:?}").trim_start_matches("Op"), fmt_num(*count)),
            chalk(fmt_pct(share)).dim()
        ));
    }
    terminal::log(row("total", fmt_num(total)));
}

pub fn print_vm_profile(profile: &VmProfile, interp_frame_share: Option<f64>) {
    terminal::blank();
    terminal::log(chalk("Inline Caches").cyan().bold());

    let rate = |hits: u64, misses: u64| -> String {
        let total = hits + misses;
        if total == 0 {
            "—".to_owned()
        } else {
            format!("{} acierto", fmt_pct(hits as f64 / total as f64))
        }
    };

    terminal::log(row_note(
        "IC hits",
        fmt_num(profile.ic_hits),
        rate(profile.ic_hits, profile.ic_misses),
    ));
    terminal::log(row("IC misses", fmt_num(profile.ic_misses)));

    let sites = [
        (
            "GetProp",
            profile.ic_hits_getprop,
            profile.ic_misses_getprop,
        ),
        (
            "SetProp",
            profile.ic_hits_setprop,
            profile.ic_misses_setprop,
        ),
        (
            "CallMethod",
            profile.ic_hits_callmethod,
            profile.ic_misses_callmethod,
        ),
    ];
    for (name, hits, misses) in sites {
        if hits == 0 && misses == 0 {
            continue;
        }
        terminal::log(row_note(
            &format!("  {name}"),
            fmt_num(hits),
            rate(hits, misses),
        ));
    }

    terminal::blank();
    terminal::log(chalk("Llamadas").cyan().bold());
    let call_total = profile.calls_vm_fast + profile.calls_prepare_slow + profile.calls_native;
    let pct = |n: u64| -> String {
        if call_total == 0 {
            "—".to_owned()
        } else {
            fmt_pct(n as f64 / call_total as f64)
        }
    };
    terminal::log(row_note(
        "vm-fast",
        fmt_num(profile.calls_vm_fast),
        pct(profile.calls_vm_fast),
    ));
    terminal::log(row_note(
        "slow/prepare",
        fmt_num(profile.calls_prepare_slow),
        pct(profile.calls_prepare_slow),
    ));
    terminal::log(row_note(
        "native",
        fmt_num(profile.calls_native),
        pct(profile.calls_native),
    ));
    terminal::log(row("total", fmt_num(call_total)));

    terminal::blank();
    terminal::log(chalk("GC").cyan().bold());
    // The three allocation counters answer different questions and have been
    // read as contradicting each other. Each says which population it counts.
    terminal::log(row_note(
        "nursery allocs",
        fmt_num(profile.nursery_allocs),
        "todo objeto joven",
    ));
    terminal::log(row_note(
        "heap allocs",
        fmt_num(profile.heap_allocs),
        "entradas en el heap",
    ));
    terminal::log(row("minor gc runs", fmt_num(profile.minor_gc_count)));
    terminal::log(row_note(
        "minor gc promoted",
        fmt_num(profile.minor_gc_promoted),
        "nursery → old",
    ));
    terminal::log(row("gc collections", fmt_num(profile.gc_collections)));
    terminal::log(row("gc freed", fmt_num(profile.gc_freed)));

    let live_share = if profile.heap_total > 0 {
        fmt_pct(profile.heap_live as f64 / profile.heap_total as f64)
    } else {
        "—".to_owned()
    };
    terminal::log(row_note(
        "heap live (post-gc)",
        fmt_num(profile.heap_live),
        format!("{live_share} de {} slots", fmt_num(profile.heap_total)),
    ));

    terminal::blank();
    let scope = match interp_frame_share {
        Some(share) => format!("[solo intérprete — {} de los frames]", fmt_pct(share)),
        None => "[solo intérprete]".to_owned(),
    };
    terminal::log(format!(
        "{} {}",
        chalk("Register VM").cyan().bold(),
        chalk(scope).dim()
    ));
    terminal::log(row("Move opcodes", fmt_num(profile.move_opcodes)));
    terminal::log(row("frame pushes", fmt_num(profile.frame_pushes)));
    terminal::log(row("frame pops", fmt_num(profile.frame_pops)));
}
