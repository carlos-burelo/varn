use varn_core::ast::{AstArena, Program};
use varn_debug_flags::DebugFlags;

use crate::error::CliError;

pub fn parse_debug_opt(spec: Option<&str>) -> Result<DebugFlags, CliError> {
    match spec {
        Some(s) => varn_debug::flags::parse_debug_flags(s)
            .map_err(|e| CliError::new(e.exit_code, e.message)),
        None => Ok(DebugFlags::default()),
    }
}

pub struct CliDebugSink;

impl varn_pipeline::DebugSink for CliDebugSink {
    fn lex(&self, tokens: &[varn_core::Token], lexeme_buf: &[u8], path: &str, debug: &DebugFlags) {
        if debug.tokens {
            varn_debug::tokens::debug_tokens(tokens, lexeme_buf, path);
        }
    }

    fn parse(
        &self,
        program: &Program,
        arena: &AstArena,
        interner: &varn_core::AtomInterner,
        debug: &DebugFlags,
    ) {
        if debug.ast {
            varn_debug::ast::debug_ast(program, arena, interner);
        }

        if debug.modules {
            varn_debug::modules::debug_modules(program, arena);
        }
    }

    fn check(
        &self,
        program: &Program,
        source: &str,
        check: &varn_checker::CheckResult,
        debug: &DebugFlags,
    ) {
        if debug.symbols {
            varn_debug::symbols::debug_symbols(check, &program.filename, debug);
        }

        if debug.check_types {
            varn_debug::expr::debug_check_types(program, source, check);
        }
    }

    fn compile(
        &self,
        program: &Program,
        arena: &AstArena,
        check: &varn_checker::CheckResult,
        proto: &varn_compiler::FunctionProto,
        graph: &varn_pipeline::module_precompile::ModuleGraphBuild,
        debug: &DebugFlags,
    ) {
        if debug.bytecode {
            varn_debug::bytecode::debug_bytecode(proto, debug);
        }

        if debug.clif {
            let helpers = varn_vm::jit::table_build::build_jit_helpers();
            varn_debug::clif::debug_clif(proto, debug, &helpers);
        }

        if debug.typeloss {
            varn_debug::typeloss::debug_typeloss(proto, debug, None);
        }

        if debug.summary {
            varn_debug::summary::debug_summary(proto);
        }

        if debug.tiers || debug.bails {
            let helpers = varn_vm::jit::table_build::build_jit_helpers();
            if debug.tiers {
                varn_debug::tiers::debug_tiers(proto, debug, &helpers, None);
            }
            if debug.bails {
                varn_debug::tiers::debug_bails(proto, debug, &helpers, None);
            }
        }

        if debug.tir || debug.tir_check {
            varn_debug::tir::debug_tir(
                program,
                arena,
                &check.bind,
                &check.expr_table,
                &check.call_mappings,
                &check.desugar,
                debug,
            );
        }

        if debug.cap_trace {
            varn_debug::debug_cap_trace(proto, &program.filename);
        }

        if debug.binds {
            varn_debug::binds::debug_binds(&program.filename);
        }

        if debug.consts {
            varn_debug::consts::debug_consts(&program.filename);
        }

        if debug.scope {
            varn_debug::scope::debug_scopes(proto, &program.filename);
        }

        if debug.graph {
            print_module_graph(graph);
        }

        if debug.bytecode {
            let mut paths: Vec<&String> = graph.modules.keys().collect();
            paths.sort_unstable();
            for path in paths {
                if path != &graph.entry_path {
                    eprintln!("\n=== MODULE BYTECODE: {} ===", path);
                    varn_debug::bytecode::debug_bytecode(&graph.modules[path], debug);
                }
            }
        }

        if debug.clif {
            let helpers = varn_vm::jit::table_build::build_jit_helpers();
            for (path, module_proto) in graph.modules.iter() {
                if path != &graph.entry_path {
                    eprintln!("\n=== MODULE CLIF: {} ===", path);
                    varn_debug::clif::debug_clif(module_proto, debug, &helpers);
                }
            }
        }

        if debug.tiers || debug.bails || debug.summary {
            let helpers = varn_vm::jit::table_build::build_jit_helpers();
            for (path, module_proto) in graph.modules.iter() {
                if path == &graph.entry_path {
                    continue;
                }
                if debug.summary {
                    eprintln!("\n=== MODULE: {} ===", path);
                    varn_debug::summary::debug_summary(module_proto);
                }

                if debug.tiers {
                    varn_debug::tiers::debug_tiers(module_proto, debug, &helpers, Some(path));
                }
                if debug.bails {
                    varn_debug::tiers::debug_bails(module_proto, debug, &helpers, Some(path));
                }
            }
        }

        if debug.typeloss {
            let mut paths: Vec<&String> = graph.modules.keys().collect();
            paths.sort_unstable();
            for path in paths {
                if path != &graph.entry_path {
                    varn_debug::typeloss::debug_typeloss(&graph.modules[path], debug, Some(path));
                }
            }
        }
    }
}

fn print_module_graph(build: &varn_pipeline::module_precompile::ModuleGraphBuild) {
    use varn_core::term::colors::{BOLD, C_MODULES, R};
    println!("\n{BOLD}Module Dependency Graph{R}");
    println!("  Entry: {C_MODULES}{}{R}", shorten_path(&build.entry_path));
    println!();
    print_graph_node(
        &build.entry_path,
        &build.deps,
        &mut rustc_hash::FxHashSet::default(),
        "",
        true,
    );
    println!();
    println!("  {} modules total", build.deps.len());
}

fn print_graph_node(
    node: &str,
    deps: &rustc_hash::FxHashMap<String, Vec<String>>,
    visited: &mut rustc_hash::FxHashSet<String>,
    prefix: &str,
    is_last: bool,
) {
    use varn_core::term::colors::{C_ERRORS, C_MODULES, R};
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
