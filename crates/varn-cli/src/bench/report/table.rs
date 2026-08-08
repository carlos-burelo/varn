//! The phase table.
//!
//! Two correctness rules govern this table, both violated by the layout it
//! replaces:
//!
//! 1. **`e2e` is not a phase.** It re-measures the whole pipeline, so its
//!    share of "the sum of phases" exceeds 100% and means nothing. It belongs
//!    below the rule, reported as an absolute with the overhead it reveals.
//! 2. **`total` has no min or max.** Summing per-phase minima answers "what if
//!    every phase hit its best in the same run", which no run did. Without
//!    paired per-run samples the honest cell is empty.

use std::time::Duration;

use varn_utilities::chalk::chalk;
use varn_utilities::terminal;

use super::fmt::{fmt_dur, fmt_pct, DurScale};
use crate::bench::stats::{PhaseStats, CV_NOISY};

/// Phases quieter than this share of the total are folded into one row unless
/// the caller asks for everything.
const FOLD_BELOW: f64 = 0.01;

pub struct TableOpts {
    /// Show every phase, including those folded as negligible.
    pub all_rows: bool,
}

pub fn print_table(phases: &[PhaseStats], e2e: Option<&PhaseStats>, opts: &TableOpts) {
    use terminal::Align::{Left, Right};

    let total_p50: Duration = phases.iter().map(|p| p.p50).sum();
    let share = |d: Duration| -> f64 {
        if total_p50.as_nanos() == 0 {
            0.0
        } else {
            d.as_nanos() as f64 / total_p50.as_nanos() as f64
        }
    };

    // One unit for the whole table: every duration cell is comparable at a
    // glance instead of carrying its own suffix.
    let scale = DurScale::for_column(
        phases
            .iter()
            .flat_map(|p| [p.min, p.p50, p.mean(), p.max])
            .chain(std::iter::once(total_p50)),
    );

    let (shown, folded): (Vec<&PhaseStats>, Vec<&PhaseStats>) = if opts.all_rows {
        (phases.iter().collect(), Vec::new())
    } else {
        phases.iter().partition(|p| share(p.p50) >= FOLD_BELOW)
    };

    let mut table = terminal::Table::new(["Phase", "min", "p50", "mean", "max", "σ/p50", "share"])
        .align([Left, Right, Right, Right, Right, Right, Right]);

    let folded_names = folded
        .iter()
        .map(|p| p.name)
        .collect::<Vec<_>>()
        .join(" · ");
    if !folded.is_empty() {
        let f_min: Duration = folded.iter().map(|p| p.min).sum();
        let f_p50: Duration = folded.iter().map(|p| p.p50).sum();
        let f_mean: Duration = folded.iter().map(|p| p.mean()).sum();
        let f_max: Duration = folded.iter().map(|p| p.max).sum();
        table.row([
            chalk("frontend").dim().bold().to_string(),
            scale.fmt(f_min),
            scale.fmt(f_p50),
            scale.fmt(f_mean),
            scale.fmt(f_max),
            chalk("—").dim().to_string(),
            chalk(fmt_pct(share(f_p50))).dim().to_string(),
        ]);
    }

    for s in shown {
        let cv = s.cv();
        let cv_cell = if cv >= CV_NOISY {
            chalk(fmt_pct(cv)).yellow().to_string()
        } else {
            chalk(fmt_pct(cv)).dim().to_string()
        };
        table.row([
            (s.color_fn)(chalk(s.name)).bold().to_string(),
            scale.fmt(s.min),
            scale.fmt(s.p50),
            chalk(scale.fmt(s.mean())).cyan().to_string(),
            scale.fmt(s.max),
            cv_cell,
            chalk(fmt_pct(share(s.p50))).dim().to_string(),
        ]);
    }

    table.rule();

    // No min/max: they are not derivable from unpaired per-phase samples.
    table.row([
        chalk("total").green().bold().to_string(),
        chalk("—").dim().to_string(),
        scale.fmt(total_p50),
        chalk(scale.fmt(phases.iter().map(|p| p.mean()).sum::<Duration>()))
            .cyan()
            .to_string(),
        chalk("—").dim().to_string(),
        String::new(),
        chalk("100.0%").dim().to_string(),
    ]);

    table.print();

    if !folded.is_empty() {
        terminal::log(format!(
            "  {}",
            chalk(format!("frontend = {folded_names}")).dim()
        ));
    }

    // `e2e` sits outside the table: it is a second measurement of the same
    // work, not a component of it.
    if let Some(e) = e2e {
        // The delta is signed. Clamping it at zero would hide the case that
        // matters most: e2e coming out *below* the sum of phases, which means
        // the phases are double-counting work.
        let delta = if e.p50 >= total_p50 {
            format!("+{}", fmt_dur(e.p50 - total_p50))
        } else {
            format!("−{}", fmt_dur(total_p50 - e.p50))
        };
        terminal::log(format!(
            "  {} {}  {}",
            chalk("e2e").cyan().bold(),
            chalk(fmt_dur(e.p50)).cyan(),
            chalk(format!(
                "(medición independiente · {delta} vs suma de fases)"
            ))
            .dim()
        ));
    }
}
