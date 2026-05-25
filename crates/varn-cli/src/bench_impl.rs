use crate::bench_output::{
    print_check_breakdown, print_opcode_hotspots, print_parse_breakdown, print_vm_profile,
};
use crate::error::CliError;
use crate::opts::DebugFlags;
use rustc_hash::FxHashMap;
use std::fs::read_to_string;
use std::rc::Rc;
use std::time::{Duration, Instant};
use varn_checker::Checker;
use varn_compiler::FunctionProto;
use varn_types::value::Closure;
use varn_vm::Vm;

const R: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const CYAN: &str = "\x1b[96m";
const YEL: &str = "\x1b[33m";
const GRN: &str = "\x1b[32m";
const MAG: &str = "\x1b[35m";
const BLU: &str = "\x1b[34m";
const WHT: &str = "\x1b[37m";
const RED: &str = "\x1b[31m";

struct PhaseStats {
    name: &'static str,
    color: &'static str,
    min: Duration,
    p50: Duration,
    max: Duration,
    total: Duration,
    stddev: Duration,
    runs: usize,
}

impl PhaseStats {
    fn from_samples(name: &'static str, color: &'static str, samples: &[Duration]) -> Self {
        let min = *samples.iter().min().expect("samples must not be empty");
        let max = *samples.iter().max().expect("samples must not be empty");
        let total: Duration = samples.iter().sum();
        let mean_ns = total.as_nanos() as f64 / samples.len() as f64;

        let mut sorted = samples.to_vec();
        sorted.sort();
        let p50 = sorted[sorted.len() / 2];

        let variance = samples
            .iter()
            .map(|s| {
                let d = s.as_nanos() as f64 - mean_ns;
                d * d
            })
            .sum::<f64>()
            / samples.len() as f64;
        let stddev = Duration::from_nanos(variance.sqrt() as u64);

        PhaseStats {
            name,
            color,
            min,
            p50,
            max,
            total,
            stddev,
            runs: samples.len(),
        }
    }

    fn mean(&self) -> Duration {
        self.total / self.runs as u32
    }
}

fn fmt_dur(d: Duration) -> String {
    let ns = d.as_nanos();
    if ns < 1_000 {
        format!("{ns} ns")
    } else if ns < 1_000_000 {
        let us = ns as f64 / 1_000.0;
        if us < 10.0 {
            fmt_f(us, 2) + " µs"
        } else if us < 100.0 {
            fmt_f(us, 1) + " µs"
        } else {
            fmt_f(us, 0) + " µs"
        }
    } else if ns < 1_000_000_000 {
        let ms = ns as f64 / 1_000_000.0;
        if ms < 10.0 {
            fmt_f(ms, 3) + " ms"
        } else if ms < 100.0 {
            fmt_f(ms, 2) + " ms"
        } else {
            fmt_f(ms, 1) + " ms"
        }
    } else {
        let s = ns as f64 / 1_000_000_000.0;
        fmt_f(s, 2) + " s"
    }
}

fn fmt_f(f: f64, decimals: usize) -> String {
    if decimals == 0 {
        return format!("{f:.0}");
    }
    let s = format!("{f:.decimals$}");
    let s = s.trim_end_matches('0');
    s.trim_end_matches('.').to_owned()
}

fn fmt_bytes(n: usize) -> String {
    if n < 1_024 {
        format!("{n} B")
    } else if n < 1_048_576 {
        format!("{:.1} KB", n as f64 / 1_024.0)
    } else {
        format!("{:.2} MB", n as f64 / 1_048_576.0)
    }
}

const W_NAME: usize = 10;
const W_TIME: usize = 9;
const W_SIG: usize = 8;
const W_PCT: usize = 6;
const SEP: &str = "─";

fn sep_line() {
    let name_col = SEP.repeat(W_NAME);
    let time_col = SEP.repeat(W_TIME);
    let sig_col = SEP.repeat(W_SIG);
    let pct_col = SEP.repeat(W_PCT);
    eprintln!(
        "  {DIM}{name_col}  {time_col}  {time_col}  {time_col}  {time_col}  {sig_col}  {time_col}  {pct_col}{R}"
    );
}

fn header_line() {
    eprintln!(
        "  {}{:<W_NAME$}  {:>W_TIME$}  {:>W_TIME$}  {:>W_TIME$}  {:>W_TIME$}  {:>W_SIG$}  {:>W_TIME$}  {:>W_PCT$}{}",
        DIM, "Phase", "min", "p50", "mean", "max", "σ", "total", "%", R
    );
}

fn phase_line(stat: &PhaseStats, share: f64) {
    let pct = format!("{:.1}%", share * 100.0);

    eprintln!(
        "  {}{}{:<W_NAME$}{}  {:>W_TIME$}  {:>W_TIME$}  {}{:>W_TIME$}{}  {:>W_TIME$}  {}{:>W_SIG$}{}  {}{:>W_TIME$}{}  {}{:>W_PCT$}{}",
        BOLD, stat.color, stat.name, R,
        fmt_dur(stat.min),
        fmt_dur(stat.p50),
        CYAN, fmt_dur(stat.mean()), R,
        fmt_dur(stat.max),
        DIM, fmt_dur(stat.stddev), R,
        DIM, fmt_dur(stat.total), R,
        DIM, pct, R,
    );
}

fn total_line(phases: &[PhaseStats]) {
    let min: Duration = phases.iter().map(|p| p.min).sum();
    let max: Duration = phases.iter().map(|p| p.max).sum();
    let total: Duration = phases.iter().map(|p| p.total).sum();
    let p50: Duration = phases.iter().map(|p| p.p50).sum();
    let runs = phases[0].runs;
    let mean = total / runs as u32;

    eprintln!(
        "  {}{}{:<W_NAME$}{}  {:>W_TIME$}  {:>W_TIME$}  {}{:>W_TIME$}{}  {:>W_TIME$}  {:>W_SIG$}  {}{:>W_TIME$}{}  {}{:>W_PCT$}{}",
        BOLD, GRN, "total", R,
        fmt_dur(min),
        fmt_dur(p50),
        CYAN, fmt_dur(mean), R,
        fmt_dur(max),
        "",
        DIM, fmt_dur(total), R,
        DIM, "100%", R,
    );
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
    runs: usize,
    debug: &DebugFlags,
    no_run: bool,
    with_output: bool,
) -> Result<(), CliError> {
    if crate::pipeline::wrc::is_wrc(path) {
        return run_bench_wrc(path, runs, no_run, with_output);
    }

    if debug.any() || no_run {
        crate::pipeline::compile_file(path, false, debug)?;
        if no_run {
            return Ok(());
        }
    }

    let source =
        read_to_string(path).map_err(|e| CliError::fatal(format!("cannot read '{path}': {e}")))?;

    let read_samples = time_n(runs, || {
        read_to_string(path).map(|_| ()).map_err(|e| e.to_string())
    })?;

    let lex_samples = time_n(runs, || {
        let _ = varn_lexer::scan(&source, path);
        Ok(())
    })?;

    let (tokens, lexeme_buf, _) = varn_lexer::scan(&source, path);
    let token_count = tokens.len();

    let tokens_ref = &tokens;
    let lexeme_buf_ref = lexeme_buf.clone();
    let parse_samples = time_n(runs, || {
        varn_parser::parse(tokens_ref.clone(), lexeme_buf_ref.clone(), path)
            .map(|_| ())
            .map_err(|errs| {
                errs.iter()
                    .map(|e| {
                        format!(
                            "{}:{}:{}: {}",
                            path, e.range.start.line, e.range.start.column, e.message
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            })
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
        let _ = Checker::check(program_ref);
        Ok(())
    })?;

    let check_result = Checker::check_with_profile(&program);

    let optimize_samples = std::cell::RefCell::new(Vec::with_capacity(runs));
    let compile_samples = time_n(runs, || {
        varn_compiler::codegen::regalloc_post::OPTIMIZE_TIME.with(|t| t.set(Duration::ZERO));
        varn_compiler::codegen::regalloc_post::OPTIMIZE_ENABLED.with(|e| e.set(true));

        let exports = varn_checker::module_resolver::resolve_module_exports_ref(
            &program_ref.filename,
            &mut vec![],
        );
        let mut export_names: Vec<std::rc::Rc<str>> = exports
            .keys()
            .map(|k| std::rc::Rc::from(k.as_str()))
            .collect();
        export_names.sort();

        let res = varn_compiler::compile_with_check_result(
            program_ref,
            &check_result.type_annotations,
            &check_result.extension_calls,
            &check_result.extension_members,
            &check_result.extension_set_members,
            export_names,
        );

        varn_compiler::codegen::regalloc_post::OPTIMIZE_ENABLED.with(|e| e.set(false));
        let opt_dur = varn_compiler::codegen::regalloc_post::OPTIMIZE_TIME.with(|t| t.get());
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

    let exports =
        varn_checker::module_resolver::resolve_module_exports_ref(&program.filename, &mut vec![]);
    let mut export_names: Vec<std::rc::Rc<str>> = exports
        .keys()
        .map(|k| std::rc::Rc::from(k.as_str()))
        .collect();
    export_names.sort();

    let proto = varn_compiler::compile_with_check_result(
        &program,
        &check_result.type_annotations,
        &check_result.extension_calls,
        &check_result.extension_members,
        &check_result.extension_set_members,
        export_names,
    )
    .map_err(|e| CliError::fatal(format!("compile error: {e}")))?;

    let precompile_start = Instant::now();
    let graph_build = crate::module_precompile::build_module_graph(&program, &source, path, &proto)
        .map_err(|e| CliError::fatal(format!("module graph build error: {e}")))?;
    let precompile_dur = precompile_start.elapsed();
    let precompiled = Rc::new(
        graph_build
            .modules
            .into_iter()
            .filter(|(module_path, _)| module_path != &graph_build.entry_path)
            .map(|(module_path, module_proto)| (module_path, Rc::new(module_proto)))
            .collect::<FxHashMap<String, Rc<FunctionProto>>>(),
    );

    let builtin_protos: Vec<FunctionProto> = crate::pipeline::builtin_protos_owned()?;

    varn_builtins::set_print_silent(true);
    varn_builtins::set_testing_silent(true);

    let mut precompiled_string_map = FxHashMap::default();
    for (name, proto) in precompiled.iter() {
        precompiled_string_map.insert(name.to_string(), proto.clone());
    }
    let optimized_precompiled_base = Rc::new(precompiled_string_map);

    let mut init_vm = Vm::new(optimized_precompiled_base.clone());
    for bp in &builtin_protos {
        let closure = Rc::new(Closure::new(Rc::new(bp.clone()), Vec::new(), Vec::new()));
        init_vm
            .run(closure)
            .map_err(|e| CliError::fatal(format!("builtin init failed: {e}")))?;
    }
    varn_builtins::set_print_silent(false);
    varn_builtins::set_testing_silent(false);

    let mut optimized_proto = proto.clone();
    init_vm.resolve_globals(&mut optimized_proto);

    let mut optimized_precompiled_map = (*optimized_precompiled_base).clone();
    for module_proto_rc in optimized_precompiled_map.values_mut() {
        init_vm.resolve_globals(Rc::make_mut(module_proto_rc));
    }
    let optimized_precompiled = Rc::new(optimized_precompiled_map);

    let (snap_globals, snap_heap) = init_vm.snapshot();

    let proto_rc = Rc::new(optimized_proto);
    let precompiled_ref = &optimized_precompiled;
    let snap_globals_ref = &snap_globals;
    let snap_heap_ref = &snap_heap;
    let exec_samples = time_n(runs, || {
        varn_builtins::reset_testing_counters();
        varn_builtins::set_print_silent(!with_output);
        varn_builtins::set_testing_silent(!with_output);

        let mut machine = Vm::from_snapshot(
            snap_globals_ref.clone(),
            snap_heap_ref.clone(),
            precompiled_ref.clone(),
        );
        let closure = Rc::new(Closure::new(proto_rc.clone(), Vec::new(), Vec::new()));
        let result = machine.run(closure).map(|_| ()).map_err(|e| e.to_string());
        varn_builtins::set_print_silent(false);
        varn_builtins::set_testing_silent(false);
        result
    })?;

    varn_vm::varn_jit::JIT_STATS.reset();
    varn_builtins::reset_testing_counters();
    varn_builtins::set_print_silent(true);
    varn_builtins::set_testing_silent(true);
    let (opcode_counts, vm_profile, jit_stats) = {
        let mut profile_vm = Vm::from_snapshot(
            snap_globals.clone(),
            snap_heap.clone(),
            optimized_precompiled.clone(),
        );
        profile_vm.enable_opcode_profiling();
        profile_vm.enable_profiling();
        let closure = Rc::new(Closure::new(proto_rc.clone(), Vec::new(), Vec::new()));
        profile_vm
            .run(closure)
            .map_err(|e| CliError::fatal(format!("profile run failed: {e}")))?;
        profile_vm.collect_gc();
        let counts = profile_vm.take_opcode_counts();
        let profile = profile_vm.take_profile();
        let stats = varn_vm::varn_jit::JIT_STATS.snapshot();
        (counts, profile, stats)
    };
    varn_builtins::set_print_silent(false);
    varn_builtins::set_testing_silent(false);

    let stats = vec![
        PhaseStats::from_samples("read", WHT, &read_samples),
        PhaseStats::from_samples("lex", YEL, &lex_samples),
        PhaseStats::from_samples("parse", GRN, &parse_samples),
        PhaseStats::from_samples("check", RED, &check_samples),
        PhaseStats::from_samples("compile", MAG, &compile_only_samples),
        PhaseStats::from_samples("optimize", YEL, &optimize_samples),
        PhaseStats::from_samples("execute", BLU, &exec_samples),
    ];

    let total_p50: Duration = stats.iter().map(|s| s.p50).sum();
    let throughput = if total_p50.as_nanos() > 0 {
        1_000_000_000.0 / total_p50.as_nanos() as f64
    } else {
        f64::INFINITY
    };

    let line_count = source.lines().count();
    let byte_count = source.len();

    eprintln!();
    eprintln!("  {BOLD}{CYAN}Benchmark{R} · {BOLD}{path}{R}  {DIM}({runs} runs){R}");
    eprintln!(
        "  {}Source  {} lines  {}  {} tokens{}",
        DIM,
        fmt_num(line_count),
        fmt_bytes(byte_count),
        fmt_num(token_count),
        R
    );
    eprintln!();
    header_line();
    sep_line();
    for s in &stats {
        let share = if total_p50.as_nanos() > 0 {
            s.p50.as_nanos() as f64 / total_p50.as_nanos() as f64
        } else {
            0.0
        };
        phase_line(s, share);
    }
    sep_line();
    total_line(&stats);
    eprintln!();
    let total_pipeline_dur: Duration = stats.iter().map(|s| s.total).sum();
    eprintln!(
        "  {}Throughput:{} {}{:.1} runs/s{}  {}(p50 end-to-end: {}){}",
        DIM,
        R,
        RED,
        throughput,
        R,
        DIM,
        fmt_dur(total_p50),
        R
    );
    eprintln!(
        "  {}Total pipeline time:{} {}{}{}",
        DIM,
        R,
        CYAN,
        fmt_dur(total_pipeline_dur),
        R
    );
    eprintln!(
        "  {}Module precompilation (cold startup):{} {}{}{}",
        DIM,
        R,
        CYAN,
        fmt_dur(precompile_dur),
        R
    );
    if !with_output {
        eprintln!("  {DIM}Execution measured with stdout muted (--withOutput to disable){R}");
    }
    print_parse_breakdown(&parse_profile);
    print_check_breakdown(&check_result.profile);
    print_opcode_hotspots(&opcode_counts);
    if let Some(ref profile) = vm_profile {
        print_vm_profile(profile);
    }
    crate::bench_output::print_jit_stats(&jit_stats);
    eprintln!();

    Ok(())
}

fn run_bench_wrc(path: &str, runs: usize, no_run: bool, with_output: bool) -> Result<(), CliError> {
    let compiled = crate::pipeline::wrc::read_wrc(path)?;
    let compile_output = crate::pipeline::cache::compile_output_from_graph(compiled)?;

    if no_run {
        return Ok(());
    }

    let file_size = std::fs::metadata(path)
        .map(|m| m.len() as usize)
        .unwrap_or(0);

    let load_samples = time_n(runs, || {
        crate::pipeline::wrc::read_wrc(path)
            .map(|_| ())
            .map_err(|e| e.message.clone())
    })?;

    let builtin_protos: Vec<varn_compiler::FunctionProto> =
        crate::pipeline::builtin_protos_owned()?;

    varn_builtins::set_print_silent(true);
    varn_builtins::set_testing_silent(true);

    let mut precompiled_map = FxHashMap::default();
    for (name, proto) in compile_output.precompiled.iter() {
        precompiled_map.insert(name.to_string(), proto.clone());
    }
    let precompiled_base = Rc::new(precompiled_map);

    let mut init_vm = Vm::new(precompiled_base.clone());
    for bp in &builtin_protos {
        let closure = Rc::new(Closure::new(Rc::new(bp.clone()), Vec::new(), Vec::new()));
        init_vm
            .run(closure)
            .map_err(|e| CliError::fatal(format!("builtin init failed: {e}")))?;
    }
    varn_builtins::set_print_silent(false);
    varn_builtins::set_testing_silent(false);

    let mut optimized_proto = compile_output.entry_proto.clone();
    init_vm.resolve_globals(&mut optimized_proto);

    let mut optimized_precompiled_map = (*precompiled_base).clone();
    for module_proto_rc in optimized_precompiled_map.values_mut() {
        init_vm.resolve_globals(Rc::make_mut(module_proto_rc));
    }
    let optimized_precompiled = Rc::new(optimized_precompiled_map);

    let (snap_globals, snap_heap) = init_vm.snapshot();

    let proto_rc = Rc::new(optimized_proto);
    let precompiled_ref = &optimized_precompiled;
    let snap_globals_ref = &snap_globals;
    let snap_heap_ref = &snap_heap;

    let exec_samples = time_n(runs, || {
        varn_builtins::reset_testing_counters();
        varn_builtins::set_print_silent(!with_output);
        varn_builtins::set_testing_silent(!with_output);
        let mut machine = Vm::from_snapshot(
            snap_globals_ref.clone(),
            snap_heap_ref.clone(),
            precompiled_ref.clone(),
        );
        let closure = Rc::new(Closure::new(proto_rc.clone(), Vec::new(), Vec::new()));
        let result = machine.run(closure).map(|_| ()).map_err(|e| e.to_string());
        varn_builtins::set_print_silent(false);
        varn_builtins::set_testing_silent(false);
        result
    })?;

    varn_vm::varn_jit::JIT_STATS.reset();
    varn_builtins::reset_testing_counters();
    varn_builtins::set_print_silent(true);
    varn_builtins::set_testing_silent(true);
    let (opcode_counts, vm_profile, jit_stats) = {
        let mut profile_vm = Vm::from_snapshot(
            snap_globals.clone(),
            snap_heap.clone(),
            optimized_precompiled.clone(),
        );
        profile_vm.enable_opcode_profiling();
        profile_vm.enable_profiling();
        let closure = Rc::new(Closure::new(proto_rc.clone(), Vec::new(), Vec::new()));
        profile_vm
            .run(closure)
            .map_err(|e| CliError::fatal(format!("profile run failed: {e}")))?;
        profile_vm.collect_gc();
        let counts = profile_vm.take_opcode_counts();
        let profile = profile_vm.take_profile();
        let stats = varn_vm::varn_jit::JIT_STATS.snapshot();
        (counts, profile, stats)
    };
    varn_builtins::set_print_silent(false);
    varn_builtins::set_testing_silent(false);

    let stats = vec![
        PhaseStats::from_samples("load", WHT, &load_samples),
        PhaseStats::from_samples("execute", BLU, &exec_samples),
    ];

    let total_p50: Duration = stats.iter().map(|s| s.p50).sum();
    let throughput = if total_p50.as_nanos() > 0 {
        1_000_000_000.0 / total_p50.as_nanos() as f64
    } else {
        f64::INFINITY
    };

    eprintln!();
    eprintln!(
        "  {BOLD}{CYAN}Benchmark{R} · {BOLD}{path}{R}  {DIM}({runs} runs)  [.vnc compiled]{R}"
    );
    eprintln!(
        "  {DIM}Binary  {}  (no source phases){R}",
        fmt_bytes(file_size)
    );
    eprintln!();
    header_line();
    sep_line();
    for s in &stats {
        let share = if total_p50.as_nanos() > 0 {
            s.p50.as_nanos() as f64 / total_p50.as_nanos() as f64
        } else {
            0.0
        };
        phase_line(s, share);
    }
    sep_line();
    total_line(&stats);
    eprintln!();
    let total_pipeline_dur: Duration = stats.iter().map(|s| s.total).sum();
    eprintln!(
        "  {DIM}Throughput:{R} {RED}{:.1} runs/s{R}  {DIM}(p50 end-to-end: {}){R}",
        throughput,
        fmt_dur(total_p50),
    );
    eprintln!(
        "  {DIM}Total pipeline time:{R} {CYAN}{}{R}",
        fmt_dur(total_pipeline_dur)
    );
    if !with_output {
        eprintln!("  {DIM}Execution measured with stdout muted (--withOutput to disable){R}");
    }
    print_opcode_hotspots(&opcode_counts);
    if let Some(ref profile) = vm_profile {
        print_vm_profile(profile);
    }
    crate::bench_output::print_jit_stats(&jit_stats);
    eprintln!();

    Ok(())
}

fn fmt_num(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}
