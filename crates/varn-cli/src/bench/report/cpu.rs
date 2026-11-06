use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::time::Duration;

use varn_vm::CpuProfile;

use super::fmt::{fmt_dur, share, short_path, truncate_middle};
use super::plain::{
    Align::{Left, Right},
    Report, Table,
};

const TOP_FUNCTIONS: usize = 30;
const TOP_EDGES: usize = 30;
const TOP_LINES: usize = 30;
const TOP_STACKS: usize = 10;
const NAME_WIDTH: usize = 48;
const FILE_WIDTH: usize = 40;

#[derive(Default)]
struct Counts {
    own: u64,
    total: u64,
}

pub fn print(report: &mut Report, profile: &CpuProfile) {
    report.title("cpu profile");
    let mut table = Table::pairs();
    table.row(["samples".to_owned(), profile.samples.to_string()]);
    table.row(["rate".to_owned(), format!("{} Hz", profile.hz)]);
    table.row(["wall".to_owned(), fmt_dur(profile.wall)]);
    table.row([
        "sampled".to_owned(),
        fmt_dur(Duration::from_secs_f64(
            profile.samples as f64 / f64::from(profile.hz),
        )),
    ]);
    table.row([
        "in compiled code".to_owned(),
        format!("{}%", share(profile.compiled_samples, profile.samples)),
    ]);
    table.row(["dropped".to_owned(), profile.dropped.to_string()]);
    table.row(["truncated stacks".to_owned(), profile.truncated.to_string()]);
    report.table(&table);

    if profile.samples == 0 {
        report.line("no samples were captured, the program ran for less than one sampling period");
        return;
    }

    functions(report, profile);
    lines(report, profile);
    edges(report, profile);
    stacks(report, profile);
}

fn functions(report: &mut Report, profile: &CpuProfile) {
    let mut counts: BTreeMap<u32, Counts> = BTreeMap::new();
    for (stack, samples) in &profile.stacks {
        if let Some(leaf) = stack.last() {
            counts.entry(*leaf).or_default().own += samples;
        }
        for id in stack.iter().collect::<BTreeSet<_>>() {
            counts.entry(*id).or_default().total += samples;
        }
    }
    let mut rows: Vec<(u32, Counts)> = counts.into_iter().collect();
    rows.sort_by_key(|(id, c)| {
        (
            std::cmp::Reverse(c.own),
            std::cmp::Reverse(c.total),
            profile.frames[*id as usize].function.clone(),
        )
    });
    report.title("cpu functions");
    let mut table = Table::new(
        ["function", "file", "self", "self%", "total", "total%"],
        [Left, Left, Right, Right, Right, Right],
    );
    for (id, c) in rows.into_iter().take(TOP_FUNCTIONS) {
        let frame = &profile.frames[id as usize];
        table.row([
            truncate_middle(&frame.function, NAME_WIDTH),
            truncate_middle(&short_path(&frame.file), FILE_WIDTH),
            c.own.to_string(),
            share(c.own, profile.samples),
            c.total.to_string(),
            share(c.total, profile.samples),
        ]);
    }
    report.table(&table);
}

fn lines(report: &mut Report, profile: &CpuProfile) {
    let mut rows: Vec<(&(u32, u32), &u64)> = profile.lines.iter().collect();
    rows.sort_by_key(|(site, samples)| (std::cmp::Reverse(**samples), **site));
    report.title("cpu lines");
    let mut table = Table::new(
        ["function", "file", "line", "self", "self%"],
        [Left, Left, Right, Right, Right],
    );
    for ((id, line), samples) in rows.into_iter().take(TOP_LINES) {
        let frame = &profile.frames[*id as usize];
        table.row([
            truncate_middle(&frame.function, NAME_WIDTH),
            truncate_middle(&short_path(&frame.file), FILE_WIDTH),
            line.to_string(),
            samples.to_string(),
            share(*samples, profile.samples),
        ]);
    }
    report.table(&table);
}

fn edges(report: &mut Report, profile: &CpuProfile) {
    let mut counts: BTreeMap<(u32, u32), u64> = BTreeMap::new();
    for (stack, samples) in &profile.stacks {
        let pairs: BTreeSet<(u32, u32)> = stack.windows(2).map(|w| (w[0], w[1])).collect();
        for pair in pairs {
            *counts.entry(pair).or_default() += samples;
        }
    }
    if counts.is_empty() {
        return;
    }
    let mut rows: Vec<((u32, u32), u64)> = counts.into_iter().collect();
    rows.sort_by_key(|(pair, samples)| (std::cmp::Reverse(*samples), *pair));
    report.title("cpu call edges");
    let mut table = Table::new(
        ["caller", "callee", "samples", "share%"],
        [Left, Left, Right, Right],
    );
    for ((caller, callee), samples) in rows.into_iter().take(TOP_EDGES) {
        table.row([
            truncate_middle(&profile.frames[caller as usize].function, NAME_WIDTH),
            truncate_middle(&profile.frames[callee as usize].function, NAME_WIDTH),
            samples.to_string(),
            share(samples, profile.samples),
        ]);
    }
    report.table(&table);
}

fn stacks(report: &mut Report, profile: &CpuProfile) {
    let mut rows: Vec<(&Vec<u32>, u64)> = profile.stacks.iter().map(|(s, n)| (s, *n)).collect();
    rows.sort_by_key(|(stack, samples)| (std::cmp::Reverse(*samples), (*stack).clone()));
    report.title("cpu hottest stacks");
    let mut table = Table::new(["samples", "share%", "stack"], [Right, Right, Left]);
    for (stack, samples) in rows.into_iter().take(TOP_STACKS) {
        table.row([
            samples.to_string(),
            share(samples, profile.samples),
            folded_path(profile, stack),
        ]);
    }
    report.table(&table);
}

fn folded_path(profile: &CpuProfile, stack: &[u32]) -> String {
    stack
        .iter()
        .map(|id| profile.frames[*id as usize].function.as_str())
        .collect::<Vec<_>>()
        .join(";")
}

pub fn folded(profile: &CpuProfile) -> String {
    let mut text = String::new();
    for (stack, samples) in &profile.stacks {
        let _ = writeln!(text, "{} {samples}", folded_path(profile, stack));
    }
    text
}
