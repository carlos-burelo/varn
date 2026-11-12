use vn_core::debug_flags::DebugFlags;
use vn_core::term::terminal;
use vn_pipeline::{CacheStatus, Compilation, DiagnosticReport, Executable};

pub fn report(compilation: &Compilation, debug: &DebugFlags, verbose: bool) {
    if verbose {
        cache_lines(&compilation.cache);
    }

    let path = compilation.source.path();

    if let Some(tokens) = &compilation.tokens {
        if verbose {
            terminal::tagged(
                "Varn",
                format_args!("scanned {} tokens", tokens.tokens.len()),
            );
        }
        if debug.tokens {
            vn_debug::tokens::debug_tokens(&tokens.tokens, &tokens.lexeme_buf, path);
        }
    }

    let Some(parsed) = &compilation.parsed else {
        return;
    };
    if verbose {
        terminal::tagged(
            "Varn",
            format_args!("parsed {} top-level statements", parsed.program.body.len()),
        );
    }
    if debug.ast
        && let Some(interner) = compilation.interner()
    {
        vn_debug::ast::debug_ast(&parsed.program, &parsed.arena, interner);
    }
    if debug.modules {
        vn_debug::modules::debug_modules(&parsed.program, &parsed.arena);
    }

    let Some(checked) = &compilation.checked else {
        return;
    };
    let source = compilation.source.text();
    if let Some(report) = compilation.warning_report() {
        print_report(&report);
    }
    for report in &compilation.dependency_warnings {
        print_report(report);
    }
    if debug.symbols {
        vn_debug::symbols::debug_symbols(checked, &parsed.program.filename, debug);
    }
    if debug.check_types {
        vn_debug::expr::debug_check_types(&parsed.program, source, checked);
    }

    let Some(executable) = &compilation.executable else {
        return;
    };
    if matches!(compilation.cache, CacheStatus::Hit) {
        return;
    }
    if verbose {
        terminal::tagged("Varn", "generating bytecode...");
        terminal::tagged("Varn", "resolving module graph...");
    }
    compiled(parsed, checked, executable, debug);

    if verbose && let CacheStatus::WriteFailed(e) = &compilation.cache {
        terminal::tagged("Varn", format_args!("compile cache write skipped: {e}"));
    }
}

fn cache_lines(status: &CacheStatus) {
    match status {
        CacheStatus::Bypassed | CacheStatus::WriteFailed(_) => {}
        CacheStatus::Hit => terminal::tagged("Varn", "compile cache hit"),
        CacheStatus::Absent => terminal::tagged("Varn", "compile cache miss"),
        CacheStatus::Stale(reason) => {
            terminal::tagged("Varn", format_args!("cache miss: {reason}"));
            terminal::tagged("Varn", "compile cache miss");
        }
        CacheStatus::Unreadable(e) => {
            terminal::tagged("Varn", format_args!("compile cache read skipped: {e}"));
            terminal::tagged("Varn", "compile cache miss");
        }
    }
}

fn print_report(report: &DiagnosticReport) {
    for diagnostic in report.diagnostics.iter() {
        terminal::log(vn_core::diagnostics::format_diagnostic(
            diagnostic,
            &report.source,
        ));
    }
}

fn compiled(
    parsed: &vn_pipeline::Parsed,
    checked: &vn_sem::output::CheckResult,
    executable: &Executable,
    debug: &DebugFlags,
) {
    let program = &parsed.program;
    let arena = &parsed.arena;
    let proto = &executable.entry_proto;
    let graph = &executable.graph_artifact;

    if debug.bytecode {
        vn_debug::bytecode::debug_bytecode(proto, debug);
    }

    if debug.clif {
        let helpers = vn_vm::jit::table_build::build_jit_helpers(&vn_vm::ExecSettings::of(
            vn_core::env::process(),
            false,
        ));
        vn_debug::clif::debug_clif(proto, debug, &helpers);
    }

    if debug.typeloss {
        vn_debug::typeloss::debug_typeloss(proto, debug, None);
    }

    if debug.summary {
        vn_debug::summary::debug_summary(proto);
    }

    if debug.tiers || debug.bails {
        let helpers = vn_vm::jit::table_build::build_jit_helpers(&vn_vm::ExecSettings::of(
            vn_core::env::process(),
            false,
        ));
        if debug.tiers {
            vn_debug::tiers::debug_tiers(proto, debug, &helpers, None);
        }
        if debug.bails {
            vn_debug::tiers::debug_bails(proto, debug, &helpers, None);
        }
    }

    if debug.tir || debug.tir_check {
        vn_debug::tir::debug_tir(
            program,
            arena,
            &checked.bind,
            &checked.expr_table,
            &checked.call_mappings,
            &checked.desugar,
            debug,
        );
    }

    if debug.cap_trace {
        vn_debug::debug_cap_trace(proto, &program.filename);
    }

    if debug.binds {
        vn_debug::binds::debug_binds(&program.filename);
    }

    if debug.consts {
        vn_debug::consts::debug_consts(&program.filename);
    }

    if debug.scope {
        vn_debug::scope::debug_scopes(proto, &program.filename);
    }

    if debug.graph {
        print_module_graph(&graph.entry_path, &executable.deps);
    }

    if debug.bytecode {
        let mut paths: Vec<&String> = graph.modules.keys().collect();
        paths.sort_unstable();
        for path in paths {
            if path != &graph.entry_path {
                eprintln!("\n=== MODULE BYTECODE: {} ===", path);
                vn_debug::bytecode::debug_bytecode(&graph.modules[path], debug);
            }
        }
    }

    if debug.clif {
        let helpers = vn_vm::jit::table_build::build_jit_helpers(&vn_vm::ExecSettings::of(
            vn_core::env::process(),
            false,
        ));
        for (path, module_proto) in graph.modules.iter() {
            if path != &graph.entry_path {
                eprintln!("\n=== MODULE CLIF: {} ===", path);
                vn_debug::clif::debug_clif(module_proto, debug, &helpers);
            }
        }
    }

    if debug.tiers || debug.bails || debug.summary {
        let helpers = vn_vm::jit::table_build::build_jit_helpers(&vn_vm::ExecSettings::of(
            vn_core::env::process(),
            false,
        ));
        for (path, module_proto) in graph.modules.iter() {
            if path == &graph.entry_path {
                continue;
            }
            if debug.summary {
                eprintln!("\n=== MODULE: {} ===", path);
                vn_debug::summary::debug_summary(module_proto);
            }

            if debug.tiers {
                vn_debug::tiers::debug_tiers(module_proto, debug, &helpers, Some(path));
            }
            if debug.bails {
                vn_debug::tiers::debug_bails(module_proto, debug, &helpers, Some(path));
            }
        }
    }

    if debug.typeloss {
        let mut paths: Vec<&String> = graph.modules.keys().collect();
        paths.sort_unstable();
        for path in paths {
            if path != &graph.entry_path {
                vn_debug::typeloss::debug_typeloss(&graph.modules[path], debug, Some(path));
            }
        }
    }
}

fn print_module_graph(entry_path: &str, deps: &rustc_hash::FxHashMap<String, Vec<String>>) {
    use vn_core::term::colors::{BOLD, C_MODULES, R};
    println!("\n{BOLD}Module Dependency Graph{R}");
    println!("  Entry: {C_MODULES}{}{R}", shorten_path(entry_path));
    println!();
    print_graph_node(
        entry_path,
        deps,
        &mut rustc_hash::FxHashSet::default(),
        "",
        true,
    );
    println!();
    println!("  {} modules total", deps.len());
}

fn print_graph_node(
    node: &str,
    deps: &rustc_hash::FxHashMap<String, Vec<String>>,
    visited: &mut rustc_hash::FxHashSet<String>,
    prefix: &str,
    is_last: bool,
) {
    use vn_core::term::colors::{C_ERRORS, C_MODULES, R};
    let connector = if is_last { "└─" } else { "├─" };
    let short = shorten_path(node);
    if visited.contains(node) {
        println!("  {prefix}{connector} {C_ERRORS}(cycle){R} {short}");
        return;
    }
    println!("  {prefix}{connector} {C_MODULES}{short}{R}");
    visited.insert(node.to_owned());

    let children = deps.get(node).map(|v| v.as_slice()).unwrap_or(&[]);
    let child_prefix = format!("{prefix}{}", if is_last { "   " } else { "│  " });
    for (i, child) in children.iter().enumerate() {
        let last = i + 1 == children.len();
        print_graph_node(child, deps, visited, &child_prefix, last);
    }
}

fn shorten_path(path: &str) -> String {
    if path.contains(':') && !path.contains('/') && !path.contains('\\') {
        return path.to_owned();
    }
    let normalized = path.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() <= 2 {
        path.to_owned()
    } else {
        format!("…/{}", parts[parts.len() - 2..].join("/"))
    }
}

pub fn natives() {
    println!("std resolved: {:?}", vn_modules::std_root::resolve());
    println!("--- NATIVE OPERATIONS ---");
    for entry in vn_builtins::dispatch::all_native_ops() {
        println!(
            "id: {} | module: {} | ns: {} | symbol: {} | capability: {} | func_ptr: {:?}",
            vn_builtins::native_op_id(
                entry.module_id(),
                entry.namespace_path(),
                entry.symbol_name()
            )
            .map_or("-".to_owned(), |id| id.to_string()),
            entry.module_id(),
            entry.namespace_path(),
            entry.symbol_name(),
            entry.capability().unwrap_or("-"),
            entry.func_ptr
        );
    }
}
