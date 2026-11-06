use std::rc::Rc;
use std::time::{Duration, Instant};

use super::detail::{cpu_profile, instrumented_run};
use super::harness::{run_vm_to_completion, time_n, time_n_freq_setup, VmFactory};
use super::output::emit;
use super::report::jit::top_blocker;
use super::report::summary::{ExecSplit, SourceInfo, Summary};
use super::report::vm::breakdown;
use super::stats::PhaseStats;
use super::BenchOpts;
use crate::error::CliError;

mod bench_pipeline;
mod e2e;

use bench_pipeline::{profile_inputs, sample_stages};

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

    let read_samples = time_n(runs, opts.warmup, || {
        if eval.is_none() {
            crate::pipeline::Source::file(path).map_err(|e| e.to_string())?;
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
    let precompile = samples.link_cold;

    let factory = VmFactory::new(
        session,
        Rc::clone(&precompiled),
        Rc::new(proto.clone()),
        !opts.show_output,
        opts.capabilities.clone(),
    )?;

    let (exec_samples, cpu_freq, tiered_during_window) = time_n_freq_setup(
        runs,
        opts.warmup,
        || {
            varn_builtins::reset_testing_counters();
            factory.build()
        },
        |machine| run_vm_to_completion(machine, factory.entry_proto()),
    )?;

    let e2e_samples = e2e::measure_e2e(session, &loaded, runs, opts.warmup, &factory)?;

    let e2e_stats = PhaseStats::from_samples("e2e (cold)", &e2e_samples);
    let phases = vec![
        PhaseStats::from_samples("read", &read_samples),
        PhaseStats::from_samples("lex", &samples.lex),
        PhaseStats::from_samples("parse", &samples.parse),
        PhaseStats::from_samples("check", &samples.check),
        PhaseStats::from_samples("compile", &samples.lower),
        PhaseStats::from_samples("optimize", &samples.optimize),
        PhaseStats::from_samples("execute (warm)", &exec_samples),
    ];
    let execute = phases.iter().find(|p| p.name == "execute (warm)");

    varn_jit::JIT_STATS.reset();
    varn_jit::stats::start_recording();
    let _ = factory.run_once_silent();
    let jit = varn_jit::JIT_STATS.snapshot();
    let records = varn_jit::stats::take_records();

    let summary = Summary {
        path,
        runs,
        bytes: source.len() as u64,
        source: Some(SourceInfo {
            lines: source.lines().count(),
            tokens: token_count,
        }),
        phases: &phases,
        e2e: Some(&e2e_stats),
        precompile: Some(precompile),
        split: execute.and_then(|e| ExecSplit::from_single_run(e.p50, &jit)),
        jit: &jit,
        top_blocker: top_blocker(&records),
        tiered_during_window,
        cpu: cpu_freq,
        silenced_output: !opts.show_output,
    };
    let profile = opts
        .profile
        .as_ref()
        .map(|profile| cpu_profile(&factory, profile))
        .transpose()?;
    emit(
        opts,
        &summary,
        summary.record(),
        profile.as_ref(),
        |report| {
            if opts.verbose {
                let phase_p50 = |name: &str| phases.iter().find(|p| p.name == name).map(|p| p.p50);
                breakdown(
                    report,
                    "parser",
                    &[
                        ("program_loop", parse_profile.program_loop),
                        ("stmt_or_decl", parse_profile.stmt_or_decl),
                        ("block", parse_profile.block),
                        ("recover", parse_profile.recover),
                    ],
                    phase_p50("parse"),
                );
                let cp = &check_result.profile;
                breakdown(
                    report,
                    "checker",
                    &[
                        ("load_globals", cp.load_globals),
                        ("bind", cp.bind),
                        ("merge_core", cp.merge_core_members),
                        ("enrich_calls", cp.enrich_call_returns),
                        ("init", cp.init),
                        ("check_stmts", cp.check_stmts),
                        ("annotations", cp.collect_annotations),
                        ("finalize", cp.finalize),
                        ("cleanup", cp.cleanup),
                    ],
                    phase_p50("check"),
                );
                instrumented_run(report, &factory, &jit, &records, "program")?;
            }
            Ok(())
        },
    )?;

    super::enforce_coverage_floor(&jit, opts.min_clif_coverage)
}
