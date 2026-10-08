use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use varn_checker::module_resolver::ImportResolver;

use rustc_hash::FxHashMap;
use varn_checker::Checker;
use varn_compiler::FunctionProto;
use varn_core::ModuleId;

use varn_core::term::chalk::chalk;
use varn_core::term::terminal;

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

mod bench_pipeline;
mod e2e;

use bench_pipeline::{export_names_of, parse_shared, verbose_sections};

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
    let session = varn_pipeline::resolver::Session::new();

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
        crate::pipeline::phase_lex(&source, path, false, &debug_flags, &varn_pipeline::NullSink)
            .map(|_| ())
            .map_err(|e| e.message)
    })?;

    let (tokens, lexeme_buf) =
        crate::pipeline::phase_lex(&source, path, false, &debug_flags, &varn_pipeline::NullSink)
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
            &varn_pipeline::NullSink,
        )
        .map(|_| ())
        .map_err(|e| format!("{e}"))
    })?;

    let (program, parse_profile, interner, arena) = parse_shared(tokens, lexeme_buf, path)
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
    let arena_ref = &arena;
    let check_samples = time_n(runs, || {
        crate::pipeline::phase_check(
            program_ref,
            arena_ref,
            interner.clone(),
            &source,
            &debug_flags,
            &session,
            &varn_pipeline::NullSink,
        )
        .map(|_| ())
        .map_err(|e| format!("{e}"))
    })?;

    let check_result = Checker::check_with(
        &program,
        &arena,
        interner,
        session.resolver(),
        varn_checker::CheckOptions::compile(),
    );

    let optimize_samples = std::cell::RefCell::new(Vec::with_capacity(runs));
    let compile_samples = time_n(runs, || {
        let (res, opt_dur) = varn_pipeline::emit_and_compile(
            program_ref,
            arena_ref,
            &check_result,
            export_names_of(&program_ref.filename, &session),
            &source,
            true,
        );
        optimize_samples.borrow_mut().push(opt_dur);

        res.map(|_| ()).map_err(|e| format!("compile failed: {e}"))
    })?;

    let optimize_samples = optimize_samples.into_inner();
    let compile_only_samples: Vec<Duration> = compile_samples
        .iter()
        .zip(&optimize_samples)
        .map(|(c, o)| c.saturating_sub(*o))
        .collect();

    let (final_result, _) = varn_pipeline::emit_and_compile(
        &program,
        &arena,
        &check_result,
        export_names_of(&program.filename, &session),
        &source,
        false,
    );
    let proto = final_result.map_err(|e| CliError::fatal(format!("compile error: {e}")))?;

    let precompile_start = Instant::now();
    let graph_build = varn_pipeline::module_precompile::build_module_graph(
        &program,
        &arena,
        &source,
        path,
        &proto,
        &check_result.bind.interner,
        &session,
    )
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
    let loader = std::sync::Arc::new(varn_pipeline::stdlib_loader::PipelineLoader::new());
    varn_builtins::set_print_silent(!opts.show_output);
    varn_builtins::set_testing_silent(!opts.show_output);
    let factory = VmFactory::new(
        Rc::clone(&precompiled),
        builtin_protos,
        loader.clone(),
        ModuleId::local_str(path),
        Rc::new(proto.clone()),
    )?;

    varn_core::term::log(format!("  running {runs} runs..."));
    let (exec_samples, cpu_freq, tiered_during_window) = time_n_freq_setup_progress(
        runs,
        || {
            varn_builtins::reset_testing_counters();
            factory.build()
        },
        |machine| run_vm_to_completion(machine, factory.entry_proto()),
        |_done, _samples| {},
    )?;

    let e2e_samples = e2e::measure_e2e(runs, eval, path, &debug_flags, &factory, &session)?;

    let e2e_stats = PhaseStats::from_samples("e2e (cold)", |c| c.cyan(), &e2e_samples);
    let phases = vec![
        PhaseStats::from_samples("read", |c| c.white(), &read_samples),
        PhaseStats::from_samples("lex", |c| c.yellow(), &lex_samples),
        PhaseStats::from_samples("parse", |c| c.green(), &parse_samples),
        PhaseStats::from_samples("check", |c| c.red(), &check_samples),
        PhaseStats::from_samples("compile", |c| c.magenta(), &compile_only_samples),
        PhaseStats::from_samples("optimize", |c| c.yellow(), &optimize_samples),
        PhaseStats::from_samples("execute (warm)", |c| c.blue(), &exec_samples),
    ];
    let total_p50: Duration = phases.iter().map(|p| p.p50).sum();
    let execute = phases.iter().find(|p| p.name == "execute (warm)");

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

    varn_builtins::set_print_silent(false);
    varn_builtins::set_testing_silent(false);

    super::enforce_coverage_floor(&exec_jit, opts.min_clif_coverage)
}
