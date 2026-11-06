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
