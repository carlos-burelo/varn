//! Benchmarking a precompiled `.vnc` bundle: no source phases, just load and
//! execute.

use std::rc::Rc;
use std::time::Duration;

use varn_core::ModuleId;
use varn_term::chalk::chalk;
use varn_term::terminal;
use varn_types::value::Closure;
use varn_vm::Vm;

use super::harness::{run_vm_to_completion, time_n, time_n_freq_setup, VmFactory};
use super::report::coverage::{print_coverage, top_blocker};
use super::report::headline::{ExecSplit, Headline};
use super::report::hotspots::print_hotspots;
use super::report::profile::{print_opcode_hotspots, print_vm_profile};
use super::report::table::{print_table, TableOpts};
use super::stats::PhaseStats;
use super::BenchOpts;
use crate::error::CliError;

pub fn run(path: &str, opts: &BenchOpts) -> Result<(), CliError> {
    let runs = opts.runs;
    let compiled = crate::pipeline::wrc::read_wrc(path)?;
    let compile_output = crate::pipeline::cache::compile_output_from_graph(compiled)?;

    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    let load_samples = time_n(runs, || {
        crate::pipeline::wrc::read_wrc(path)
            .map(|_| ())
            .map_err(|e| e.message.clone())
    })?;

    let builtin_protos: Vec<varn_compiler::FunctionProto> = crate::pipeline::core_protos_owned()?;

    varn_builtins::set_print_silent(true);
    varn_builtins::set_testing_silent(true);

    let precompiled_base = compile_output.precompiled.clone();

    let loader = std::sync::Arc::new(varn_vm::loader::CompositeLoader::new(vec![
        Box::new(varn_pipeline::stdlib_loader::FileLoader),
        Box::new(varn_pipeline::stdlib_loader::StdlibLoader),
    ]));

    let settings = varn_vm::ExecSettings::from_env(false);
    let mut init_vm = Vm::new(precompiled_base.clone(), settings).with_loader(loader.clone());
    for bp in &builtin_protos {
        let closure = Rc::new(Closure::new(Rc::new(bp.clone()), Vec::new(), Vec::new()));
        init_vm
            .run(closure)
            .map_err(|e| CliError::fatal(format!("builtin init failed: {e}")))?;
    }
    varn_builtins::set_print_silent(!opts.show_output);
    varn_builtins::set_testing_silent(!opts.show_output);

    let mut optimized_proto = compile_output.entry_proto.clone();
    init_vm.resolve_globals(&mut optimized_proto);

    // Pre-bind the module map to this store so the per-run `eval_module_proto`
    // hits its already-resolved check instead of rewriting every module inside
    // the timed region. Correctness still comes from that check, not from here.
    let mut optimized_precompiled_map = (*precompiled_base).clone();
    for module_proto_rc in optimized_precompiled_map.values_mut() {
        init_vm.resolve_globals(Rc::make_mut(module_proto_rc));
    }
    let optimized_precompiled = Rc::new(optimized_precompiled_map);

    init_vm.ctx.run_minor_gc();
    init_vm.collect_gc();
    varn_vm::prefill_native_modules(&mut init_vm);

    let (snap_globals, snap_heap, snap_modules) = init_vm.snapshot();

    let factory = VmFactory {
        globals: snap_globals,
        heap: snap_heap,
        precompiled: optimized_precompiled,
        modules: snap_modules,
        loader: loader.clone(),
        module_id: ModuleId::local_str(path),
        proto: Rc::new(optimized_proto),
    };

    let (exec_samples, cpu_freq) = time_n_freq_setup(
        runs,
        || {
            varn_builtins::reset_testing_counters();
            factory.build()
        },
        |machine| run_vm_to_completion(machine, factory.closure()),
    )?;

    let phases = vec![
        PhaseStats::from_samples("load", |c| c.white(), &load_samples),
        PhaseStats::from_samples("execute", |c| c.blue(), &exec_samples),
    ];
    let total_p50: Duration = phases.iter().map(|p| p.p50).sum();
    let execute = phases.iter().find(|p| p.name == "execute");

    // One instrumented run supplies every JIT figure, so compile time stays
    // comparable to a single execute p50. See `source.rs` for why averaging a
    // multi-run snapshot is wrong.
    let (exec_jit, records) = {
        varn_vm::varn_jit::JIT_STATS.reset();
        varn_vm::varn_jit::stats::start_recording();
        varn_builtins::set_print_silent(true);
        varn_builtins::set_testing_silent(true);
        let _ = factory.run_once();
        varn_builtins::set_print_silent(!opts.show_output);
        varn_builtins::set_testing_silent(!opts.show_output);
        (
            varn_vm::varn_jit::JIT_STATS.snapshot(),
            varn_vm::varn_jit::stats::take_records(),
        )
    };

    Headline {
        path,
        runs,
        source_lines: 0,
        source_bytes: file_size,
        tokens: 0,
        e2e: None,
        execute,
        total_p50,
        split: execute.and_then(|e| ExecSplit::from_single_run(e.p50, &exec_jit)),
        jit: Some(&exec_jit),
        coverage_scope: "bundle .vnc",
        top_blocker: top_blocker(&records),
        cpu: cpu_freq,
    }
    .print();

    terminal::blank();
    print_table(
        &phases,
        None,
        &TableOpts {
            all_rows: opts.all_rows,
        },
    );

    if !opts.show_output {
        terminal::blank();
        terminal::log(
            chalk("  Ejecución medida con stdout silenciado (--show-output para verlo)").dim(),
        );
    }

    if opts.verbose {
        varn_builtins::reset_testing_counters();
        varn_builtins::set_print_silent(true);
        varn_builtins::set_testing_silent(true);

        let mut profile_vm = factory.build();
        profile_vm.enable_opcode_profiling();
        profile_vm.enable_profiling();
        profile_vm.enable_hotspot_profiling();
        run_vm_to_completion(&mut profile_vm, factory.closure())
            .map_err(|e| CliError::fatal(format!("profile run failed: {e}")))?;
        profile_vm.collect_gc();
        let opcode_counts = profile_vm.take_opcode_counts();
        let mut vm_profile = profile_vm.take_profile();
        let hotspots = profile_vm.take_hotspots();

        varn_builtins::set_print_silent(!opts.show_output);
        varn_builtins::set_testing_silent(!opts.show_output);

        print_coverage(&exec_jit, &records, "bundle .vnc");

        let interp_share = (exec_jit.total_frames() > 0).then(|| exec_jit.never_compiled_ratio());
        print_opcode_hotspots(&opcode_counts, interp_share);
        if let Some(ref mut profile) = vm_profile {
            let move_count = opcode_counts
                .iter()
                .find(|(op, _)| matches!(op, varn_core::OpCode::Move))
                .map(|(_, n)| *n)
                .unwrap_or(0);
            profile.move_opcodes = move_count;
            print_vm_profile(profile, interp_share);
        }
        if let Some(ref hs) = hotspots {
            print_hotspots(hs);
        }
        terminal::blank();
    }

    varn_builtins::set_print_silent(false);
    varn_builtins::set_testing_silent(false);

    // Same placement as `source::run`: report first, verdict after.
    super::enforce_coverage_floor(&exec_jit, opts.min_clif_coverage)
}
