use super::harness::{run_vm_to_completion, VmFactory};
use super::report::hotspots::hotspots;
use super::report::jit::coverage;
use super::report::plain::Report;
use super::report::vm::{counters, opcodes};
use crate::error::CliError;

pub fn instrumented_run(
    report: &mut Report,
    factory: &VmFactory,
    jit: &varn_jit::JitStatsSnapshot,
    records: &[varn_jit::CompileRecord],
    scope: &str,
) -> Result<(), CliError> {
    varn_builtins::reset_testing_counters();

    let mut machine = factory.build_silent();
    machine.enable_opcode_profiling();
    machine.enable_profiling();
    machine.enable_hotspot_profiling();
    run_vm_to_completion(&mut machine, factory.entry_proto())
        .map_err(|e| CliError::fatal(format!("profile run failed: {e}")))?;
    machine.collect_gc();
    let opcode_counts = machine.take_opcode_counts();
    let vm_profile = machine.take_profile();
    let hotspot_counters = machine.take_hotspots();

    coverage(report, jit, records, scope);

    let interp_share = (jit.total_frames() > 0).then(|| jit.never_compiled_ratio());
    opcodes(report, &opcode_counts, interp_share);
    if let Some(mut profile) = vm_profile {
        profile.move_opcodes = opcode_counts
            .iter()
            .find(|(op, _)| matches!(op, varn_core::OpCode::Move))
            .map_or(0, |(_, n)| *n);
        counters(report, &profile);
    }
    if let Some(found) = hotspot_counters {
        hotspots(report, &found);
    }
    Ok(())
}

const PROFILE_TARGET_SAMPLES: u64 = 4_000;
const PROFILE_MAX_RUNS: usize = 2_000;
const PROFILE_MAX_WALL: std::time::Duration = std::time::Duration::from_secs(10);

pub fn cpu_profile(
    factory: &VmFactory,
    opts: &super::ProfileOpts,
) -> Result<varn_vm::CpuProfile, CliError> {
    let mut profile = varn_vm::CpuProfile::new(opts.hz);
    let started = std::time::Instant::now();
    for _ in 0..PROFILE_MAX_RUNS {
        varn_builtins::reset_testing_counters();
        let mut machine = factory.build_silent();
        let entry = factory.entry_proto();
        let profiler = varn_vm::CpuProfiler::start(&mut machine, &mut profile, &entry)
            .map_err(CliError::fatal)?;
        let outcome = run_vm_to_completion(&mut machine, entry);
        profiler.finish(&mut profile);
        outcome.map_err(|e| CliError::fatal(format!("profile run failed: {e}")))?;
        if profile.samples >= PROFILE_TARGET_SAMPLES || started.elapsed() >= PROFILE_MAX_WALL {
            break;
        }
    }
    if let Some(path) = &opts.out {
        std::fs::write(path, super::report::cpu::folded(&profile))
            .map_err(|e| CliError::fatal(format!("could not write {path}: {e}")))?;
    }
    Ok(profile)
}
