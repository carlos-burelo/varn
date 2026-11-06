use varn_vm::CpuProfile;

use super::record::{compare, Change, FunctionRecord, ProfileRecord, Record, Verdict};
use super::report::cpu;
use super::report::fmt::fmt_dur;
use super::report::plain::{
    Align::{Left, Right},
    Report, Table,
};
use super::report::summary::Summary;
use super::{BenchOpts, Format};
use crate::error::CliError;

const RECORDED_FUNCTIONS: usize = 30;
const GATED_PHASE: &str = "execute (warm)";

pub fn emit(
    opts: &BenchOpts,
    summary: &Summary<'_>,
    mut record: Record,
    profile: Option<&CpuProfile>,
    details: impl FnOnce(&mut Report) -> Result<(), CliError>,
) -> Result<(), CliError> {
    record.profile = profile.map(profile_record);
    let changes = match &opts.baseline {
        Some(path) => compare(&Record::load(path)?, &record),
        None => Vec::new(),
    };
    record.comparison = changes.clone();

    match opts.format {
        Format::Json => println!("{}", record.json()),
        Format::Text => {
            let mut report = Report::new();
            summary.fill(&mut report);
            details(&mut report)?;
            if let Some(profile) = profile {
                cpu::print(&mut report, profile);
            }
            changes_table(&mut report, &changes);
            report.print();
        }
    }
    summary.warn();
    gate(opts, &changes)
}

fn gate(opts: &BenchOpts, changes: &[Change]) -> Result<(), CliError> {
    let Some(limit) = opts.max_regression else {
        return Ok(());
    };
    let Some(change) = changes.iter().find(|c| c.phase == GATED_PHASE) else {
        return Err(CliError::fatal(format!(
            "the baseline has no '{GATED_PHASE}' phase to compare"
        )));
    };
    if change.verdict == Verdict::Regression && change.delta_percent > limit {
        return Err(CliError::fatal(format!(
            "{GATED_PHASE} is {:.1}% slower than the baseline, the limit is {limit:.1}%",
            change.delta_percent
        )));
    }
    Ok(())
}

fn changes_table(report: &mut Report, changes: &[Change]) {
    if changes.is_empty() {
        return;
    }
    report.title("baseline");
    let mut table = Table::new(
        ["phase", "baseline", "current", "delta%", "verdict"],
        [Left, Right, Right, Right, Left],
    );
    for change in changes {
        table.row([
            change.phase.clone(),
            fmt_dur(std::time::Duration::from_nanos(change.baseline_p50_ns)),
            fmt_dur(std::time::Duration::from_nanos(change.current_p50_ns)),
            format!("{:+.1}", change.delta_percent),
            match change.verdict {
                Verdict::Regression => "regression",
                Verdict::Improvement => "improvement",
                Verdict::Same => "same",
            }
            .to_owned(),
        ]);
    }
    report.table(&table);
}

fn profile_record(profile: &CpuProfile) -> ProfileRecord {
    ProfileRecord {
        hz: profile.hz,
        samples: profile.samples,
        compiled_samples: profile.compiled_samples,
        functions: cpu::rank_functions(profile)
            .into_iter()
            .take(RECORDED_FUNCTIONS)
            .map(|(id, counts)| {
                let frame = &profile.frames[id as usize];
                FunctionRecord {
                    function: frame.function.clone(),
                    file: frame.file.clone(),
                    self_samples: counts.own,
                    total_samples: counts.total,
                }
            })
            .collect(),
    }
}
