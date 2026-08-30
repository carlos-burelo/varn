//! Benchmarking a `.vn` source file through the full pipeline.

use varn_checker::module_resolver::ImportResolver;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rustc_hash::FxHashMap;
use varn_checker::Checker;
use varn_compiler::FunctionProto;
use varn_core::ModuleId;
use std::io::Write as IoWrite;

use varn_core::term::chalk::chalk;
use varn_core::term::terminal;
use varn_types::value::Closure;
use varn_vm::Vm;

use super::harness::{run_vm_to_completion, time_n, time_n_freq_setup_progress, VmFactory};
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

pub fn run(path: &str, eval: Option<&str>, opts: &BenchOpts) -> Result<(), CliError> {
    let runs = opts.runs;
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
    let debug_flags = varn_debug::flags::DebugFlags::default();

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
        crate::pipeline::phase_lex(&source, path, false, &debug_flags)
            .map(|_| ())
            .map_err(|e| e.message)
    })?;

    let (tokens, lexeme_buf) = crate::pipeline::phase_lex(&source, path, false, &debug_flags)
        .map_err(|e| CliError::fatal(e.message))?;
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
            &debug_flags,
        )
        .map(|_| ())
        .map_err(|e| format!("{e}"))
    })?;

    let (program, parse_profile) = varn_parser::parse_with_profile(tokens, lexeme_buf, path)
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


    let program_ref = &program;
    let check_samples = time_n(runs, || {
        crate::pipeline::phase_check(program_ref, &source, &debug_flags, false)
            .map(|_| ())
            .map_err(|e| format!("{e}"))
    })?;

    // The COMPILE configuration, deliberately. This used to call
    // `check_with_profile`, which was `check_for_lsp` under another name: the
    // reported breakdown described a check that also built the per-expression
    // type table, which a real compile never builds. `profile` is filled by
    // every check, so nothing is lost by asking for the right one.
    let check_result = varn_pipeline::resolver::with_resolver(|r| {
        Checker::check_with(&program, r, varn_checker::CheckOptions::compile())
    });

    let optimize_samples = std::cell::RefCell::new(Vec::with_capacity(runs));
    let compile_samples = time_n(runs, || {
        varn_compiler::regalloc::regalloc_post::OPTIMIZE_TIME.with(|t| t.set(Duration::ZERO));
        varn_compiler::regalloc::regalloc_post::OPTIMIZE_ENABLED.with(|e| e.set(true));

        let res = varn_compiler::compile_module(
            program_ref,
            &check_result.type_annotations,
            &check_result.extension_calls,
            &check_result.extension_members,
            &check_result.extension_set_members,
            export_names_of(&program_ref.filename),
        );

        varn_compiler::regalloc::regalloc_post::OPTIMIZE_ENABLED.with(|e| e.set(false));
        let opt_dur = varn_compiler::regalloc::regalloc_post::OPTIMIZE_TIME.with(|t| t.get());
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

    let proto = varn_compiler::compile_module(
        &program,
        &check_result.type_annotations,
        &check_result.extension_calls,
        &check_result.extension_members,
        &check_result.extension_set_members,
        export_names_of(&program.filename),
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

    let loader = std::sync::Arc::new(varn_vm::loader::CompositeLoader::new(vec![
        Box::new(varn_pipeline::stdlib_loader::FileLoader),
        Box::new(varn_pipeline::stdlib_loader::StdlibLoader),
    ]));

    let settings = varn_vm::ExecSettings::from_env(false);
    let mut init_vm = Vm::new(precompiled.clone(), settings).with_loader(loader.clone());
    for bp in &builtin_protos {
        let closure = Rc::new(Closure::new(Rc::new(bp.clone()), Vec::new(), Vec::new()));
        init_vm
            .run(closure)
            .map_err(|e| CliError::fatal(format!("builtin init failed: {e}")))?;
    }
    varn_builtins::set_print_silent(!opts.show_output);
    varn_builtins::set_testing_silent(!opts.show_output);

    varn_builtins::set_print_silent(true);
    varn_builtins::set_testing_silent(true);
    varn_vm::prefill_native_modules(&mut init_vm);
    varn_builtins::set_print_silent(!opts.show_output);
    varn_builtins::set_testing_silent(!opts.show_output);

    let mut optimized_proto = proto.clone();
    init_vm.resolve_globals(&mut optimized_proto);

    // Pre-bind the module map to this store so the per-run `eval_module_proto`
    // hits its already-resolved check instead of rewriting every module inside
    // the timed region. Correctness still comes from that check, not from here.
    let mut optimized_precompiled_map = (*precompiled).clone();
    for module_proto_rc in optimized_precompiled_map.values_mut() {
        init_vm.resolve_globals(Rc::make_mut(module_proto_rc));
    }
    let optimized_precompiled = Rc::new(optimized_precompiled_map);

    init_vm.ctx.run_minor_gc();
    init_vm.collect_gc();

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

    const PROG_BAR: usize = 32;
    let (exec_samples, cpu_freq) = time_n_freq_setup_progress(
        runs,
        || {
            varn_builtins::reset_testing_counters();
            factory.build()
        },
        |machine| run_vm_to_completion(machine, factory.closure()),
        |done, samples| {
            let filled = (done * PROG_BAR / runs.max(1)).min(PROG_BAR);
            let bar = format!(
                "\x1b[36m{}\x1b[2m{}\x1b[0m",
                "█".repeat(filled),
                "░".repeat(PROG_BAR - filled)
            );
            let mut sorted = samples.to_vec();
            sorted.sort();
            let p50_ns = sorted[sorted.len() / 2].as_nanos();
            let p50_str = if p50_ns < 1_000 {
                format!("{p50_ns}ns")
            } else if p50_ns < 1_000_000 {
                format!("{}µs", p50_ns / 1_000)
            } else {
                format!("{:.1}ms", p50_ns as f64 / 1_000_000.0)
            };
            print!("\r  \x1b[2mexecute\x1b[0m  [{bar}]  {done}/{runs}  \x1b[2mp50 {p50_str}\x1b[0m  ");
            let _ = std::io::stdout().flush();
        },
    )?;
    print!("\r\x1b[2K");
    let _ = std::io::stdout().flush();

    let e2e_samples = time_n(runs, || {
        let source = match eval {
            Some(code) => code.to_owned(),
            None => crate::pipeline::read_source_file(path).map_err(|e| e.message.clone())?,
        };

        let (tokens, lexeme_buf) = crate::pipeline::phase_lex(&source, path, false, &debug_flags)
            .map_err(|e| e.message)?;

        let (program, _) =
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

        let check_result = varn_pipeline::resolver::with_resolver(|r| {
            Checker::check_with(&program, r, varn_checker::CheckOptions::compile())
        });

        let mut proto = varn_compiler::compile_module(
            &program,
            &check_result.type_annotations,
            &check_result.extension_calls,
            &check_result.extension_members,
            &check_result.extension_set_members,
            export_names_of(&program.filename),
        )
        .map_err(|e| format!("compile failed: {}", e))?;

        varn_builtins::reset_testing_counters();

        let mut machine = factory.build();

        // Real runs resolve global names to indices before executing
        // (pipeline::execute); without this the e2e phase measures an
        // unresolved-globals interpreter path no user ever hits.
        machine.resolve_globals(&mut proto);
        let closure = Rc::new(Closure::new(Rc::new(proto), Vec::new(), Vec::new()));
        run_vm_to_completion(&mut machine, closure)
    })?;

    let e2e_stats = PhaseStats::from_samples("e2e", |c| c.cyan(), &e2e_samples);
    let phases = vec![
        PhaseStats::from_samples("read", |c| c.white(), &read_samples),
        PhaseStats::from_samples("lex", |c| c.yellow(), &lex_samples),
        PhaseStats::from_samples("parse", |c| c.green(), &parse_samples),
        PhaseStats::from_samples("check", |c| c.red(), &check_samples),
        PhaseStats::from_samples("compile", |c| c.magenta(), &compile_only_samples),
        PhaseStats::from_samples("optimize", |c| c.yellow(), &optimize_samples),
        PhaseStats::from_samples("execute", |c| c.blue(), &exec_samples),
    ];
    let total_p50: Duration = phases.iter().map(|p| p.p50).sum();
    let execute = phases.iter().find(|p| p.name == "execute");

    // One instrumented run supplies every JIT figure. Taking them from the
    // timed window instead would mix per-run quantities (compile time, which
    // must be comparable to one execute p50) with cumulative ones (function
    // counts across warmup plus every iteration).
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
        source_lines: source.lines().count(),
        source_bytes: source.len() as u64,
        tokens: token_count,
        e2e: Some(&e2e_stats),
        execute,
        total_p50,
        split: execute.and_then(|e| ExecSplit::from_single_run(e.p50, &exec_jit)),
        jit: Some(&exec_jit),
        coverage_scope: "programa completo",
        top_blocker: top_blocker(&records),
        cpu: cpu_freq,
        phases: Some(&phases),
        e2e_samples: Some(&e2e_samples),
    }
    .print();

    terminal::log(format!(
        "  {}{}  {}",
        chalk("precompilación: ").dim(),
        chalk(super::report::fmt::fmt_dur(precompile_dur)).cyan().dim(),
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

    varn_builtins::set_print_silent(false);
    varn_builtins::set_testing_silent(false);

    // Last, so the report is on screen even when the guard fails: a threshold
    // breach is something to read the numbers about, not instead of.
    super::enforce_coverage_floor(&exec_jit, opts.min_clif_coverage)
}

fn export_names_of(filename: &str) -> Vec<Rc<str>> {
    let exports = varn_pipeline::resolver::with_resolver(|r| r.module_exports(filename, &mut vec![]));
    let mut names: Vec<Rc<str>> = exports.keys().map(|k| Rc::from(k.as_str())).collect();
    names.sort();
    names
}

#[allow(clippy::too_many_arguments)]
fn verbose_sections(
    factory: &VmFactory,
    exec_jit: &varn_vm::varn_jit::JitStatsSnapshot,
    records: &[varn_vm::varn_jit::CompileRecord],
    parse_profile: &varn_parser::ParseProfile,
    check_result: &varn_checker::CheckResult,
    phases: &[PhaseStats],
    opts: &BenchOpts,
) -> Result<(), CliError> {
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

    let breakdown = BreakdownOpts {
        all_rows: opts.all_rows,
    };
    let phase_p50 = |name: &str| phases.iter().find(|p| p.name == name).map(|p| p.p50);

    print_breakdown(
        "Parser Breakdown",
        |c| c.green(),
        &[
            ("program_loop", parse_profile.program_loop),
            ("stmt_or_decl", parse_profile.stmt_or_decl),
            ("block", parse_profile.block),
            ("recover", parse_profile.recover),
        ],
        phase_p50("parse"),
        &breakdown,
    );

    let cp = &check_result.profile;
    print_breakdown(
        "Checker Breakdown",
        |c| c.red(),
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
        &breakdown,
    );

    print_coverage(exec_jit, records, "programa completo");

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
    Ok(())
}
