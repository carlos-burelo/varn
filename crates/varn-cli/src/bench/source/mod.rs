use std::rc::Rc;
use std::time::{Duration, Instant};

use varn_core::term::chalk::chalk;
use varn_core::term::terminal;

use super::harness::{run_vm_to_completion, time_n, time_n_freq_setup, VmFactory};
use super::report::coverage::{print_coverage, top_blocker};
use super::report::headline::{ExecSplit, Headline};
use super::report::hotspots::print_hotspots;
use super::report::profile::{
    print_breakdown, print_opcode_hotspots, print_vm_profile, BreakdownOpts,
};
use super::report::table::{print_table, TableOpts};
use super::stats::PhaseStats;
use super::BenchOpts;
use crate::error::CliError;

mod bench_pipeline;
mod e2e;

use bench_pipeline::{profile_inputs, sample_stages, verbose_sections};

pub fn run(
    session: &varn_pipeline::Session,
    path: &str,
    eval: Option<&str>,
    opts: &BenchOpts,
) -> Result<(), CliError> {
    let runs = opts.runs;
    let loaded = match eval {
        Some(code) => crate::pipeline::Source::virtual_source(crate::source_name::EVAL, code),
        None => crate::pipeline::Source::file(path)?,
    };
    let path = loaded.path();
    let source = loaded.text();

    let read_samples = time_n(runs, || {
        match eval {
            Some(code) => {
                let _ = code.to_owned();
            }
            None => {
                crate::pipeline::Source::file(path).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    })?;

    let (samples, mut last) = sample_stages(session, &loaded, runs)?;
    let varn_pipeline::Executable {
        entry_proto: proto,
        precompiled,
        ..
    } = last
        .executable
        .take()
        .ok_or_else(|| CliError::fatal("the compilation produced no executable"))?;
    let token_count = last.tokens.as_ref().map_or(0, |t| t.tokens.len());
    let (parse_profile, check_result) = profile_inputs(&last, path)?;
    let precompile_dur = samples.link_cold;

    let factory = VmFactory::new(
        session,
        Rc::clone(&precompiled),
        Rc::new(proto.clone()),
        !opts.show_output,
        opts.capabilities.clone(),
    )?;

    varn_core::term::log(format!("  running {runs} runs..."));
    let (exec_samples, cpu_freq, tiered_during_window) = time_n_freq_setup(
        runs,
        || {
            varn_builtins::reset_testing_counters();
            factory.build()
        },
        |machine| run_vm_to_completion(machine, factory.entry_proto()),
        |_done, _samples| {},
    )?;

    let e2e_samples = e2e::measure_e2e(session, &loaded, runs, &factory)?;

    let e2e_stats = PhaseStats::from_samples("e2e (cold)", |c| c.cyan(), &e2e_samples);
    let phases = vec![
        PhaseStats::from_samples("read", |c| c.white(), &read_samples),
        PhaseStats::from_samples("lex", |c| c.yellow(), &samples.lex),
        PhaseStats::from_samples("parse", |c| c.green(), &samples.parse),
        PhaseStats::from_samples("check", |c| c.red(), &samples.check),
        PhaseStats::from_samples("compile", |c| c.magenta(), &samples.lower),
        PhaseStats::from_samples("optimize", |c| c.yellow(), &samples.optimize),
        PhaseStats::from_samples("execute (warm)", |c| c.blue(), &exec_samples),
    ];
    let total_p50: Duration = phases.iter().map(|p| p.p50).sum();
    let execute = phases.iter().find(|p| p.name == "execute (warm)");

    let (exec_jit, records) = {
        varn_jit::JIT_STATS.reset();
        varn_jit::stats::start_recording();
        let _ = factory.run_once_silent();
        (
            varn_jit::JIT_STATS.snapshot(),
            varn_jit::stats::take_records(),
        )
    };

    Headline {
        path,
        runs,
        source_lines: source.lines().count(),
        source_bytes: source.len() as u64,
        tokens: token_count,
        e2e: Some(&e2e_stats),
        execute,
        total_p50,
        split: execute.and_then(|e| ExecSplit::from_single_run(e.p50, &exec_jit)),
        jit: Some(&exec_jit),
        top_blocker: top_blocker(&records),
        tiered_during_window,
        cpu: cpu_freq,
        phases: Some(&phases),
    }
    .print();

    terminal::log(format!(
        "  {}{}  {}",
        chalk("precompilación: ").dim(),
        chalk(super::report::fmt::fmt_dur(precompile_dur))
            .cyan()
            .dim(),
        chalk("(costo de arranque en frío)").dim()
    ));
    if !opts.show_output {
        terminal::log(
            chalk("  Ejecución medida con stdout silenciado (--show-output para verlo)").dim(),
        );
    }

    if opts.verbose {
        terminal::blank();
        print_table(
            &phases,
            Some(&e2e_stats),
            &TableOpts {
                all_rows: opts.all_rows,
            },
        );
        verbose_sections(
            &factory,
            &exec_jit,
            &records,
            &parse_profile,
            &check_result,
            &phases,
            opts,
        )?;
    }

    super::enforce_coverage_floor(&exec_jit, opts.min_clif_coverage)
}
