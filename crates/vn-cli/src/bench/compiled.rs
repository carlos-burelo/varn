use std::rc::Rc;

use super::detail::{cpu_profile, instrumented_run};
use super::harness::{run_vm_to_completion, time_n, time_n_freq_setup, VmFactory};
use super::output::emit;
use super::report::jit::top_blocker;
use super::report::summary::{ExecSplit, Summary};
use super::stats::PhaseStats;
use super::BenchOpts;
use crate::error::CliError;

pub fn run(session: &vn_pipeline::Session, path: &str, opts: &BenchOpts) -> Result<(), CliError> {
    let runs = opts.runs;
    let compiled = crate::pipeline::portable::read_portable(path)?;
    let compile_output = crate::pipeline::Executable::from_graph(compiled)?;

    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    let load_samples = time_n(runs, opts.warmup, || {
        crate::pipeline::portable::read_portable(path)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })?;

    let factory = VmFactory::new(
        session,
        compile_output.precompiled.clone(),
        Rc::new(compile_output.entry_proto.clone()),
        !opts.show_output,
        opts.capabilities.clone(),
    )?;

    let (exec_samples, cpu_freq, tiered_during_window) = time_n_freq_setup(
        runs,
        opts.warmup,
        || {
            vn_builtins::reset_testing_counters();
            factory.build()
        },
        |machine| run_vm_to_completion(machine, factory.entry_proto()),
    )?;

    let phases = vec![
        PhaseStats::from_samples("load", &load_samples),
        PhaseStats::from_samples("execute (warm)", &exec_samples),
    ];
    let execute = phases.iter().find(|p| p.name == "execute (warm)");

    vn_jit::JIT_STATS.reset();
    vn_jit::stats::start_recording();
    let _ = factory.run_once_silent();
    let jit = vn_jit::JIT_STATS.snapshot();
    let records = vn_jit::stats::take_records();

    let summary = Summary {
        path,
        runs,
        bytes: file_size,
        source: None,
        phases: &phases,
        e2e: None,
        precompile: None,
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
                instrumented_run(report, &factory, &jit, &records, "bundle")?;
            }
            Ok(())
        },
    )?;

    super::enforce_coverage_floor(&jit, opts.min_clif_coverage)
}
