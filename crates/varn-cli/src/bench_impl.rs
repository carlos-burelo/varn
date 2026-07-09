use crate::bench_hotspots::print_hotspots;
use crate::bench_output::{
    print_check_breakdown, print_opcode_hotspots, print_parse_breakdown, print_vm_profile,
};
use crate::bench_phase::{fmt_bytes, fmt_dur, fmt_num, print_table, PhaseStats};
use crate::error::CliError;
use crate::profiling::JitTimers;
use rustc_hash::FxHashMap;
use std::fs::File;
use std::io::Write;
use std::rc::Rc;
use std::time::{Duration, Instant};
use varn_checker::Checker;
use varn_core::ModuleId;
use varn_opt::FunctionProto;
use varn_types::value::Closure;
use varn_utilities::chalk::chalk;
use varn_utilities::terminal;
use varn_vm::Vm;

fn run_vm_to_completion(machine: &mut Vm, closure: Rc<Closure>) -> Result<(), String> {
    loop {
        let res = machine.run(closure.clone());
        match res {
            Ok(_) => match machine.ctx.vm_suspend.take() {
                None => break,
                Some(varn_vm::exec::VmSuspend::Await { value, dest_reg }) => {
                    let resolved = match value {
                        varn_types::Value::Task(lazy) => {
                            let handle = machine.ctx.run_lazy_task_sync(lazy.as_ref());
                            match handle.peek_state() {
                                varn_types::TaskState::Resolved(v) => v,
                                varn_types::TaskState::Rejected(e) => {
                                    return Err(format!("awaited task failed: {}", e));
                                }
                                _ => varn_types::Value::Null,
                            }
                        }
                        varn_types::Value::TaskHandle(handle) => match handle.peek_state() {
                            varn_types::TaskState::Resolved(v) => v,
                            _ => match varn_vm::exec::ExecCtx::wait_task_handle(handle.clone()) {
                                Ok(v) => v,
                                Err(e) => return Err(format!("awaited task failed: {}", e)),
                            },
                        },
                        other => other,
                    };
                    let resolved_nv = machine.ctx.heap.intern(resolved);

                    if let Some(frame) = machine.ctx.frames.last() {
                        let base = frame.base;
                        let slot = base + dest_reg as usize;
                        if slot < machine.ctx.stack.len() {
                            machine.ctx.stack[slot] = resolved_nv;
                        }
                    }
                }
                Some(varn_vm::exec::VmSuspend::Task(_task)) => {}
                Some(varn_vm::exec::VmSuspend::Yield { .. }) => {}
            },
            Err(e) => {
                let mut msg = format!("runtime error: {}", e.message);
                for frame in &e.frames {
                    msg.push_str(&format!(
                        "\n  at {} ({}:{})",
                        frame.fn_name, frame.file, frame.line
                    ));
                }
                return Err(msg);
            }
        }
    }
    Ok(())
}

fn time_n<F: Fn() -> Result<(), String>>(runs: usize, f: F) -> Result<Vec<Duration>, CliError> {
    f().map_err(|e| CliError::fatal(format!("bench warmup failed: {e}")))?;

    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        f().map_err(|e| CliError::fatal(format!("bench run failed: {e}")))?;
        samples.push(start.elapsed());
    }
    Ok(samples)
}

pub fn run_bench(
    path: &str,
    eval: Option<&str>,
    runs: usize,
    show_output: bool,
    verbose: bool,
) -> Result<(), CliError> {
    if eval.is_none() && crate::pipeline::wrc::is_wrc(path) {
        return run_bench_wrc(path, runs, show_output, verbose);
    }

    let canonical;
    let path = if eval.is_some() {
        "(eval)"
    } else {
        canonical = crate::pipeline::canonicalize_path(path)?;
        canonical.as_str()
    };

    let source = match eval {
        Some(code) => code.to_owned(),
        None => crate::pipeline::read_source_file(path)?,
    };

    let read_samples = time_n(runs, || {
        match eval {
            Some(code) => {
                let _ = code.to_owned();
            }
            None => {
                crate::pipeline::read_source_file(path).map_err(|e| e.message.clone())?;
            }
        }
        Ok(())
    })?;

    let lex_samples = time_n(runs, || {
        let _ = crate::pipeline::phase_lex(
            &source,
            path,
            false,
            &varn_debug::flags::DebugFlags::default(),
        );
        Ok(())
    })?;

    let (tokens, lexeme_buf) = crate::pipeline::phase_lex(
        &source,
        path,
        false,
        &varn_debug::flags::DebugFlags::default(),
    );
    let token_count = tokens.len();

    let tokens_ref = &tokens;
    let lexeme_buf_ref = lexeme_buf.clone();
    let parse_samples = time_n(runs, || {
        crate::pipeline::phase_parse(
            tokens_ref.clone(),
            lexeme_buf_ref.clone(),
            &source,
            path,
            false,
            &varn_debug::flags::DebugFlags::default(),
        )
        .map(|_| ())
        .map_err(|e| format!("{e}"))
    })?;

    let (mut program, parse_profile) = varn_parser::parse_with_profile(tokens, lexeme_buf, path)
        .map_err(|errs| {
            let msgs: Vec<String> = errs
                .iter()
                .map(|e| {
                    format!(
                        "{}:{}:{}: {}",
                        path, e.range.start.line, e.range.start.column, e.message
                    )
                })
                .collect();
            CliError::fatal(format!("parse errors:\n{}", msgs.join("\n")))
        })?;

    varn_core::assign_ast_ids(&mut program);

    let program_ref = &program;
    let check_samples = time_n(runs, || {
        crate::pipeline::phase_check(
            program_ref,
            &source,
            &varn_debug::flags::DebugFlags::default(),
            false,
        )
        .map(|_| ())
        .map_err(|e| format!("{e}"))
    })?;

    let check_result = Checker::check_with_profile(&program);

    let optimize_samples = std::cell::RefCell::new(Vec::with_capacity(runs));
    let timers = JitTimers::new();
    let compile_samples = time_n(runs, || {
        timers.compile.start();
        varn_backend::regalloc_post::OPTIMIZE_TIME.with(|t| t.set(Duration::ZERO));
        varn_backend::regalloc_post::OPTIMIZE_ENABLED.with(|e| e.set(true));

        let exports = varn_checker::module_resolver::resolve_module_exports_ref(
            &program_ref.filename,
            &mut vec![],
        );
        let mut export_names: Vec<std::rc::Rc<str>> = exports
            .keys()
            .map(|k| std::rc::Rc::from(k.as_str()))
            .collect();
        export_names.sort();

        let res = varn_opt::compile_module(
            program_ref,
            &check_result.type_annotations,
            &check_result.extension_calls,
            &check_result.extension_members,
            &check_result.extension_set_members,
            export_names,
        );

        timers.compile.stop();

        varn_backend::regalloc_post::OPTIMIZE_ENABLED.with(|e| e.set(false));
        let opt_dur = varn_backend::regalloc_post::OPTIMIZE_TIME.with(|t| t.get());
        optimize_samples.borrow_mut().push(opt_dur);

        res.map(|_| ())
            .map_err(|e| format!("compile failed: {}", e))
    })?;

    let optimize_samples = optimize_samples.into_inner();
    let compile_only_samples: Vec<Duration> = compile_samples
        .iter()
        .zip(&optimize_samples)
        .map(|(c, o)| c.saturating_sub(*o))
        .collect();

    timers.optimize.stop();

    let exports =
        varn_checker::module_resolver::resolve_module_exports_ref(&program.filename, &mut vec![]);
    let mut export_names: Vec<std::rc::Rc<str>> = exports
        .keys()
        .map(|k| std::rc::Rc::from(k.as_str()))
        .collect();
    export_names.sort();

    let proto = varn_opt::compile_module(
        &program,
        &check_result.type_annotations,
        &check_result.extension_calls,
        &check_result.extension_members,
        &check_result.extension_set_members,
        export_names,
    )
    .map_err(|e| CliError::fatal(format!("compile error: {e}")))?;

    let precompile_start = Instant::now();
    let graph_build =
        varn_pipeline::module_precompile::build_module_graph(&program, &source, path, &proto)
            .map_err(|e| CliError::fatal(format!("module graph build error: {e}")))?;
    let precompile_dur = precompile_start.elapsed();
    let precompiled = Rc::new(
        graph_build
            .modules
            .into_iter()
            .filter(|(module_path, _)| module_path != &graph_build.entry_path)
            .map(|(module_path, module_proto)| {
                (
                    ModuleId::from_canonical_str(&module_path),
                    Rc::new(module_proto),
                )
            })
            .collect::<FxHashMap<ModuleId, Rc<FunctionProto>>>(),
    );

    let builtin_protos: Vec<FunctionProto> = crate::pipeline::core_protos_owned()?;

    varn_builtins::set_print_silent(true);
    varn_builtins::set_testing_silent(true);

    let optimized_precompiled_base = precompiled.clone();

    let loader = std::sync::Arc::new(varn_vm::loader::CompositeLoader::new(vec![
        Box::new(varn_pipeline::stdlib_loader::FileLoader),
        Box::new(varn_pipeline::stdlib_loader::StdlibLoader),
    ]));

    let mut init_vm = Vm::new(optimized_precompiled_base.clone()).with_loader(loader.clone());
    for bp in &builtin_protos {
        let closure = Rc::new(Closure::new(Rc::new(bp.clone()), Vec::new(), Vec::new()));
        init_vm
            .run(closure)
            .map_err(|e| CliError::fatal(format!("builtin init failed: {e}")))?;
    }
    varn_builtins::set_print_silent(!show_output);
    varn_builtins::set_testing_silent(!show_output);

    let mut optimized_proto = proto.clone();
    init_vm.resolve_globals(&mut optimized_proto);

    let mut optimized_precompiled_map = (*optimized_precompiled_base).clone();
    for module_proto_rc in optimized_precompiled_map.values_mut() {
        init_vm.resolve_globals(Rc::make_mut(module_proto_rc));
    }
    let optimized_precompiled = Rc::new(optimized_precompiled_map);

    let proto_rc = Rc::new(optimized_proto);

    varn_builtins::set_print_silent(true);
    varn_builtins::set_testing_silent(true);
    varn_vm::prefill_native_modules(&mut init_vm);
    varn_builtins::set_print_silent(!show_output);
    varn_builtins::set_testing_silent(!show_output);

    init_vm.ctx.run_minor_gc();
    init_vm.collect_gc();

    let (snap_globals, snap_heap, snap_modules) = init_vm.snapshot();
    let precompiled_ref = &optimized_precompiled;
    let snap_globals_ref = &snap_globals;
    let snap_heap_ref = &snap_heap;
    let snap_modules_ref = &snap_modules;
    let exec_samples = time_n(runs, || {
        timers.execute.start();
        varn_builtins::reset_testing_counters();

        let mut machine = Vm::from_snapshot(
            snap_globals_ref.clone(),
            snap_heap_ref.clone(),
            precompiled_ref.clone(),
            snap_modules_ref.clone(),
        )
        .with_loader(loader.clone());
        let main_module_id = ModuleId::local_str(path);
        let mut export_map = FxHashMap::default();
        for (idx, name) in proto_rc.export_names.iter().enumerate() {
            export_map.insert(name.clone(), idx);
        }
        let mut module_obj =
            varn_types::ModuleObj::new(main_module_id.clone(), proto_rc.export_names.len());
        module_obj.export_map = export_map;
        let module_val = machine.ctx.heap.alloc_module(Rc::new(module_obj));
        machine.ctx.modules.insert(main_module_id, module_val);
        machine.ctx.module_exports.insert(0, module_val);

        let closure = Rc::new(Closure::new(proto_rc.clone(), Vec::new(), Vec::new()));
        let res = run_vm_to_completion(&mut machine, closure);

        timers.execute.stop();
        res
    })?;

    let e2e_samples = time_n(runs, || {
        let source = match eval {
            Some(code) => code.to_owned(),
            None => crate::pipeline::read_source_file(path).map_err(|e| e.message.clone())?,
        };

        let (tokens, lexeme_buf) = crate::pipeline::phase_lex(
            &source,
            path,
            false,
            &varn_debug::flags::DebugFlags::default(),
        );

        let (mut program, _) =
            varn_parser::parse_with_profile(tokens, lexeme_buf, path).map_err(|errs| {
                let msgs: Vec<String> = errs
                    .iter()
                    .map(|e| {
                        format!(
                            "{}:{}:{}: {}",
                            path, e.range.start.line, e.range.start.column, e.message
                        )
                    })
                    .collect();
                format!("parse errors:\n{}", msgs.join("\n"))
            })?;

        varn_core::assign_ast_ids(&mut program);

        let check_result = Checker::check_with_profile(&program);

        let exports = varn_checker::module_resolver::resolve_module_exports_ref(
            &program.filename,
            &mut vec![],
        );
        let mut export_names: Vec<std::rc::Rc<str>> = exports
            .keys()
            .map(|k| std::rc::Rc::from(k.as_str()))
            .collect();
        export_names.sort();

        let mut proto = varn_opt::compile_module(
            &program,
            &check_result.type_annotations,
            &check_result.extension_calls,
            &check_result.extension_members,
            &check_result.extension_set_members,
            export_names,
        )
        .map_err(|e| format!("compile failed: {}", e))?;

        varn_builtins::reset_testing_counters();

        let mut machine = Vm::from_snapshot(
            snap_globals_ref.clone(),
            snap_heap_ref.clone(),
            precompiled_ref.clone(),
            snap_modules_ref.clone(),
        )
        .with_loader(loader.clone());

        // Real runs resolve global names to indices before executing
        // (pipeline::execute); without this the e2e phase measures an
        // unresolved-globals interpreter path no user ever hits.
        machine.resolve_globals(&mut proto);
        let proto_rc = Rc::new(proto);

        let main_module_id = ModuleId::local_str(path);
        let mut export_map = FxHashMap::default();
        for (idx, name) in proto_rc.export_names.iter().enumerate() {
            export_map.insert(name.clone(), idx);
        }
        let mut module_obj =
            varn_types::ModuleObj::new(main_module_id.clone(), proto_rc.export_names.len());
        module_obj.export_map = export_map;
        let module_val = machine.ctx.heap.alloc_module(Rc::new(module_obj));
        machine.ctx.modules.insert(main_module_id, module_val);
        machine.ctx.module_exports.insert(0, module_val);

        let closure = Rc::new(Closure::new(proto_rc.clone(), Vec::new(), Vec::new()));
        run_vm_to_completion(&mut machine, closure)
    })?;

    let e2e_stats = PhaseStats::from_samples("e2e", |c| c.cyan(), &e2e_samples);

    let stats = vec![
        PhaseStats::from_samples("read", |c| c.white(), &read_samples),
        PhaseStats::from_samples("lex", |c| c.yellow(), &lex_samples),
        PhaseStats::from_samples("parse", |c| c.green(), &parse_samples),
        PhaseStats::from_samples("check", |c| c.red(), &check_samples),
        PhaseStats::from_samples("compile", |c| c.magenta(), &compile_only_samples),
        PhaseStats::from_samples("optimize", |c| c.yellow(), &optimize_samples),
        PhaseStats::from_samples("execute", |c| c.blue(), &exec_samples),
    ];

    let total_p50: Duration = stats.iter().map(|s| s.p50).sum();
    let e2e_p50 = e2e_stats.p50;
    let throughput = if e2e_p50.as_nanos() > 0 {
        1_000_000_000.0 / e2e_p50.as_nanos() as f64
    } else {
        f64::INFINITY
    };

    let line_count = source.lines().count();
    let byte_count = source.len();

    terminal::blank();
    terminal::log(format!(
        "  {} · {}  {}",
        chalk("Benchmark").cyan().bold(),
        chalk(path).bold(),
        chalk(format!("({runs} runs)")).dim()
    ));
    terminal::log(format!(
        "  {}",
        chalk(format!(
            "Source  {} lines  {}  {} tokens",
            fmt_num(line_count),
            fmt_bytes(byte_count),
            fmt_num(token_count)
        ))
        .dim()
    ));
    terminal::blank();
    print_table(&stats, total_p50, Some(&e2e_stats));
    terminal::blank();
    let total_pipeline_dur: Duration = stats.iter().map(|s| s.total).sum();
    terminal::log(format!(
        "  {}Throughput:{} {}  {}",
        chalk("").dim(),
        "",
        chalk(format!("{:.1} runs/s", throughput)).red(),
        chalk(format!(
            "(p50: {} true e2e · {} sum phases)",
            fmt_dur(e2e_p50),
            fmt_dur(total_p50)
        ))
        .dim()
    ));
    terminal::log(format!(
        "  {}Total pipeline time:{} {}  {}",
        chalk("").dim(),
        "",
        chalk(fmt_dur(total_pipeline_dur)).cyan(),
        chalk("(sum of phases)").dim()
    ));
    terminal::log(format!(
        "  {}Module precompilation (cold startup):{} {}",
        chalk("").dim(),
        "",
        chalk(fmt_dur(precompile_dur)).cyan()
    ));
    let cold_start = precompile_dur + e2e_p50;
    let cold_throughput = if cold_start.as_nanos() > 0 {
        1_000_000_000.0 / cold_start.as_nanos() as f64
    } else {
        f64::INFINITY
    };
    terminal::log(format!(
        "  {}Cold-start throughput:{} {}  {}",
        chalk("").dim(),
        "",
        chalk(format!("{:.1} runs/s", cold_throughput)).yellow(),
        chalk(format!("(precompile + p50: {})", fmt_dur(cold_start))).dim()
    ));
    if !show_output {
        terminal::log(
            chalk("  Execution measured with stdout muted (--show-output to disable)").dim(),
        );
    }
    if verbose {
        varn_vm::varn_jit::JIT_STATS.reset();
        varn_builtins::reset_testing_counters();
        varn_builtins::set_print_silent(true);
        varn_builtins::set_testing_silent(true);

        let (opcode_counts, mut vm_profile, jit_stats, hotspots) = {
            let mut profile_vm = Vm::from_snapshot(
                snap_globals.clone(),
                snap_heap.clone(),
                optimized_precompiled.clone(),
                snap_modules.clone(),
            )
            .with_loader(loader.clone());
            let main_module_id = ModuleId::local_str(path);
            let mut export_map = FxHashMap::default();
            for (idx, name) in proto_rc.export_names.iter().enumerate() {
                export_map.insert(name.clone(), idx);
            }
            let mut module_obj =
                varn_types::ModuleObj::new(main_module_id.clone(), proto_rc.export_names.len());
            module_obj.export_map = export_map;
            let module_val = profile_vm.ctx.heap.alloc_module(Rc::new(module_obj));
            profile_vm.ctx.modules.insert(main_module_id, module_val);
            profile_vm.ctx.module_exports.insert(0, module_val);

            profile_vm.enable_opcode_profiling();
            profile_vm.enable_profiling();
            profile_vm.enable_hotspot_profiling();
            let closure = Rc::new(Closure::new(proto_rc.clone(), Vec::new(), Vec::new()));
            run_vm_to_completion(&mut profile_vm, closure)
                .map_err(|e| CliError::fatal(format!("profile run failed: {e}")))?;
            profile_vm.collect_gc();
            let counts = profile_vm.take_opcode_counts();
            let profile = profile_vm.take_profile();
            let stats = varn_vm::varn_jit::JIT_STATS.snapshot();
            let hs = profile_vm.take_hotspots();
            (counts, profile, stats, hs)
        };
        varn_builtins::set_print_silent(!show_output);
        varn_builtins::set_testing_silent(!show_output);

        if let Some(ref profile) = vm_profile {
            if let Ok(mut f) = File::create("target/debug/vm_profile.txt") {
                let _ = write!(f, "{:?}", profile);
            }
        }
        if let Ok(mut f) = File::create("target/debug/opcode_counts.txt") {
            let _ = write!(f, "{:?}", opcode_counts);
        }

        if let Ok(mut f) = File::create("target/debug/jit_profile.csv") {
            let _ = write!(
                f,
                "phase,seconds\nread,{}\nlex,{}\nparse,{}\ncheck,{}\ncompile,{}\noptimize,{}\nexecute,{}\n",
                timers.read.elapsed().as_secs_f64(),
                timers.lex.elapsed().as_secs_f64(),
                timers.parse.elapsed().as_secs_f64(),
                timers.check.elapsed().as_secs_f64(),
                timers.compile.elapsed().as_secs_f64(),
                timers.optimize.elapsed().as_secs_f64(),
                timers.execute.elapsed().as_secs_f64(),
            );
        }

        print_parse_breakdown(&parse_profile);
        print_check_breakdown(&check_result.profile);
        print_opcode_hotspots(&opcode_counts);
        if let Some(ref mut profile) = vm_profile {
            let move_count = opcode_counts
                .iter()
                .find(|(op, _)| matches!(op, varn_core::OpCode::Move))
                .map(|(_, n)| *n)
                .unwrap_or(0);
            profile.move_opcodes = move_count;
            print_vm_profile(profile);
        }
        crate::bench_output::print_jit_stats(&jit_stats);
        if let Some(ref hs) = hotspots {
            print_hotspots(hs);
        }
        terminal::blank();
    }
    varn_builtins::set_print_silent(false);
    varn_builtins::set_testing_silent(false);

    Ok(())
}

fn run_bench_wrc(
    path: &str,
    runs: usize,
    show_output: bool,
    verbose: bool,
) -> Result<(), CliError> {
    let compiled = crate::pipeline::wrc::read_wrc(path)?;
    let compile_output = crate::pipeline::cache::compile_output_from_graph(compiled)?;

    let file_size = std::fs::metadata(path)
        .map(|m| m.len() as usize)
        .unwrap_or(0);

    let load_samples = time_n(runs, || {
        crate::pipeline::wrc::read_wrc(path)
            .map(|_| ())
            .map_err(|e| e.message.clone())
    })?;

    let builtin_protos: Vec<varn_opt::FunctionProto> = crate::pipeline::core_protos_owned()?;

    varn_builtins::set_print_silent(true);
    varn_builtins::set_testing_silent(true);

    let precompiled_base = compile_output.precompiled.clone();

    let loader = std::sync::Arc::new(varn_vm::loader::CompositeLoader::new(vec![
        Box::new(varn_pipeline::stdlib_loader::FileLoader),
        Box::new(varn_pipeline::stdlib_loader::StdlibLoader),
    ]));

    let mut init_vm = Vm::new(precompiled_base.clone()).with_loader(loader.clone());
    for bp in &builtin_protos {
        let closure = Rc::new(Closure::new(Rc::new(bp.clone()), Vec::new(), Vec::new()));
        init_vm
            .run(closure)
            .map_err(|e| CliError::fatal(format!("builtin init failed: {e}")))?;
    }
    varn_builtins::set_print_silent(!show_output);
    varn_builtins::set_testing_silent(!show_output);

    let mut optimized_proto = compile_output.entry_proto.clone();
    init_vm.resolve_globals(&mut optimized_proto);

    let mut optimized_precompiled_map = (*precompiled_base).clone();
    for module_proto_rc in optimized_precompiled_map.values_mut() {
        init_vm.resolve_globals(Rc::make_mut(module_proto_rc));
    }
    let optimized_precompiled = Rc::new(optimized_precompiled_map);

    init_vm.ctx.run_minor_gc();
    init_vm.collect_gc();

    varn_vm::prefill_native_modules(&mut init_vm);

    let (snap_globals, snap_heap, snap_modules) = init_vm.snapshot();

    let proto_rc = Rc::new(optimized_proto);
    let precompiled_ref = &optimized_precompiled;
    let snap_globals_ref = &snap_globals;
    let snap_heap_ref = &snap_heap;
    let snap_modules_ref = &snap_modules;

    let exec_samples = time_n(runs, || {
        varn_builtins::reset_testing_counters();
        let mut machine = Vm::from_snapshot(
            snap_globals_ref.clone(),
            snap_heap_ref.clone(),
            precompiled_ref.clone(),
            snap_modules_ref.clone(),
        )
        .with_loader(loader.clone());
        let main_module_id = ModuleId::local_str(path);
        let mut export_map = FxHashMap::default();
        for (idx, name) in proto_rc.export_names.iter().enumerate() {
            export_map.insert(name.clone(), idx);
        }
        let mut module_obj =
            varn_types::ModuleObj::new(main_module_id.clone(), proto_rc.export_names.len());
        module_obj.export_map = export_map;
        let module_val = machine.ctx.heap.alloc_module(Rc::new(module_obj));
        machine.ctx.modules.insert(main_module_id, module_val);
        machine.ctx.module_exports.insert(0, module_val);

        let closure = Rc::new(Closure::new(proto_rc.clone(), Vec::new(), Vec::new()));
        run_vm_to_completion(&mut machine, closure)
    })?;

    let stats = vec![
        PhaseStats::from_samples("load", |c| c.white(), &load_samples),
        PhaseStats::from_samples("execute", |c| c.blue(), &exec_samples),
    ];

    let total_p50: Duration = stats.iter().map(|s| s.p50).sum();
    let throughput = if total_p50.as_nanos() > 0 {
        1_000_000_000.0 / total_p50.as_nanos() as f64
    } else {
        f64::INFINITY
    };

    terminal::blank();
    terminal::log(format!(
        "  {} · {}  {}",
        chalk("Benchmark").cyan().bold(),
        chalk(path).bold(),
        chalk(format!("({runs} runs)  [.vnc compiled]")).dim()
    ));
    terminal::log(format!(
        "  {}",
        chalk(format!(
            "Binary  {}  (no source phases)",
            fmt_bytes(file_size)
        ))
        .dim()
    ));
    terminal::blank();
    print_table(&stats, total_p50, None);
    terminal::blank();
    let total_pipeline_dur: Duration = stats.iter().map(|s| s.total).sum();
    terminal::log(format!(
        "  {}Throughput:{} {}  {}",
        chalk("").dim(),
        "",
        chalk(format!("{:.1} runs/s", throughput)).red(),
        chalk(format!("(p50 end-to-end: {})", fmt_dur(total_p50))).dim()
    ));
    terminal::log(format!(
        "  {}Total pipeline time:{} {}",
        chalk("").dim(),
        "",
        chalk(fmt_dur(total_pipeline_dur)).cyan()
    ));
    if !show_output {
        terminal::log(
            chalk("  Execution measured with stdout muted (--show-output to disable)").dim(),
        );
    }
    if verbose {
        varn_vm::varn_jit::JIT_STATS.reset();
        varn_builtins::reset_testing_counters();
        varn_builtins::set_print_silent(true);
        varn_builtins::set_testing_silent(true);
        let (opcode_counts, mut vm_profile, jit_stats, hotspots) = {
            let mut profile_vm = Vm::from_snapshot(
                snap_globals.clone(),
                snap_heap.clone(),
                optimized_precompiled.clone(),
                snap_modules.clone(),
            )
            .with_loader(loader.clone());
            let main_module_id = ModuleId::local_str(path);
            let mut export_map = FxHashMap::default();
            for (idx, name) in proto_rc.export_names.iter().enumerate() {
                export_map.insert(name.clone(), idx);
            }
            let mut module_obj =
                varn_types::ModuleObj::new(main_module_id.clone(), proto_rc.export_names.len());
            module_obj.export_map = export_map;
            let module_val = profile_vm.ctx.heap.alloc_module(Rc::new(module_obj));
            profile_vm.ctx.modules.insert(main_module_id, module_val);
            profile_vm.ctx.module_exports.insert(0, module_val);

            profile_vm.enable_opcode_profiling();
            profile_vm.enable_profiling();
            profile_vm.enable_hotspot_profiling();
            let closure = Rc::new(Closure::new(proto_rc.clone(), Vec::new(), Vec::new()));
            run_vm_to_completion(&mut profile_vm, closure)
                .map_err(|e| CliError::fatal(format!("profile run failed: {e}")))?;
            profile_vm.collect_gc();
            let counts = profile_vm.take_opcode_counts();
            let profile = profile_vm.take_profile();
            let stats = varn_vm::varn_jit::JIT_STATS.snapshot();
            let hs = profile_vm.take_hotspots();
            (counts, profile, stats, hs)
        };
        varn_builtins::set_print_silent(!show_output);
        varn_builtins::set_testing_silent(!show_output);

        print_opcode_hotspots(&opcode_counts);
        if let Some(ref mut profile) = vm_profile {
            let move_count = opcode_counts
                .iter()
                .find(|(op, _)| matches!(op, varn_core::OpCode::Move))
                .map(|(_, n)| *n)
                .unwrap_or(0);
            profile.move_opcodes = move_count;
            print_vm_profile(profile);
        }
        crate::bench_output::print_jit_stats(&jit_stats);
        if let Some(ref hs) = hotspots {
            print_hotspots(hs);
        }
        terminal::blank();
    }

    varn_builtins::set_print_silent(false);
    varn_builtins::set_testing_silent(false);

    Ok(())
}
