use varn_checker::module_resolver::ImportResolver;
use super::check::CheckResult;
use crate::PipelineError;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_compiler::FunctionProto;
use varn_core::ast::Program;
use varn_debug::flags::DebugFlags;
use varn_types::ModuleGraphArtifact;

pub const CACHE_FORMAT_VERSION: u32 = varn_modules::artifact::BUILD_FINGERPRINT;

type PipelineResult<T> = Result<T, PipelineError>;

pub struct CompileOutput {
    pub entry_proto: FunctionProto,
    pub precompiled: Rc<FxHashMap<varn_core::ModuleId, Rc<FunctionProto>>>,
    pub graph_artifact: ModuleGraphArtifact,
}

pub fn compile(
    program: &Program,
    source: &str,
    check_result: CheckResult,
    verbose: bool,
    debug: &DebugFlags,
) -> PipelineResult<CompileOutput> {
    if verbose {
        varn_core::term::terminal::tagged("Varn", "generating bytecode...");
    }

    let exports =
        crate::resolver::with_resolver(|r| r.module_exports(&program.filename, &mut vec![]));
    let mut export_names: Vec<std::rc::Rc<str>> = exports
        .keys()
        .map(|k| std::rc::Rc::from(k.as_str()))
        .collect();
    export_names.sort();

    let proto = varn_compiler::compile_module(
        program,
        &check_result.checker_result.type_annotations,
        &check_result.checker_result.extension_calls,
        &check_result.checker_result.extension_members,
        &check_result.checker_result.extension_set_members,
        export_names,
    )
    .map_err(|e| {
        PipelineError::fatal(format!(
            "{}: {e}",
            varn_core::term::chalk::chalk("error[emit]").red().bold()
        ))
    })?;

    if debug.bytecode {
        varn_debug::bytecode::debug_bytecode(&proto, debug);
    }

    if debug.clif {
        let helpers = varn_vm::jit::helpers::build_jit_helpers();
        varn_debug::clif::debug_clif(&proto, debug, &helpers);
    }

    if debug.summary {
        varn_debug::summary::debug_summary(&proto);
    }

    if debug.tiers || debug.bails || debug.roots {
        let helpers = varn_vm::jit::helpers::build_jit_helpers();
        if debug.tiers {
            varn_debug::tiers::debug_tiers(&proto, debug, &helpers, None);
        }
        if debug.bails {
            varn_debug::tiers::debug_bails(&proto, debug, &helpers, None);
        }
        if debug.roots {
            varn_debug::roots::debug_roots(&proto, debug, &helpers, None);
        }
    }

    if debug.hir {
        varn_debug::hir::debug_hir(
            program,
            &check_result.checker_result.type_annotations,
            &check_result.checker_result.extension_calls,
            &check_result.checker_result.extension_members,
            &check_result.checker_result.extension_set_members,
        );
    }

    if debug.ssa {
        varn_debug::ssa::debug_ssa(
            program,
            &check_result.checker_result.type_annotations,
            &check_result.checker_result.extension_calls,
            &check_result.checker_result.extension_members,
            &check_result.checker_result.extension_set_members,
        );
    }

    if debug.suspend {
        varn_debug::suspend::debug_suspend(
            program,
            &check_result.checker_result.type_annotations,
            &check_result.checker_result.extension_calls,
            &check_result.checker_result.extension_members,
            &check_result.checker_result.extension_set_members,
        );
    }

    if debug.cap_trace {
        varn_debug::debug_cap_trace(&proto, &program.filename);
    }

    if debug.binds {
        varn_debug::binds::debug_binds(&program.filename);
    }

    if debug.consts {
        varn_debug::consts::debug_consts(&program.filename);
    }

    if debug.scope {
        varn_debug::scope::debug_scopes(&proto, &program.filename);
    }

    if verbose {
        varn_core::term::terminal::tagged("Varn", "resolving module graph...");
    }
    let graph_build =
        crate::module_precompile::build_module_graph(program, source, &program.filename, &proto)
            .map_err(|e| PipelineError::fatal(format!("module graph error: {e}")))?;

    if debug.graph {
        print_module_graph(&graph_build);
    }

    if debug.bytecode {
        // `graph_build.modules` es un `std::collections::HashMap`: su orden de
        // iteración se siembra al azar en cada arranque de proceso, así que sin
        // ordenar aquí el volcado sale con los módulos barajados de una corrida
        // a otra. El bytecode en sí no cambia — sólo su presentación —, pero eso
        // basta para que `diff` sobre dos volcados sea inservible como oráculo.
        let mut paths: Vec<&String> = graph_build.modules.keys().collect();
        paths.sort_unstable();
        for path in paths {
            if path != &graph_build.entry_path {
                eprintln!("\n=== MODULE BYTECODE: {} ===", path);
                varn_debug::bytecode::debug_bytecode(&graph_build.modules[path], debug);
            }
        }
    }

    if debug.clif {
        let helpers = varn_vm::jit::helpers::build_jit_helpers();
        for (path, module_proto) in graph_build.modules.iter() {
            if path != &graph_build.entry_path {
                eprintln!("\n=== MODULE CLIF: {} ===", path);
                varn_debug::clif::debug_clif(module_proto, debug, &helpers);
            }
        }
    }

    // Coverage is a whole-program property, so the imported modules matter as
    // much as the entry one: a metric that stops at the entry module reports
    // a number that looks like coverage and is not.
    if debug.tiers || debug.bails || debug.summary {
        let helpers = varn_vm::jit::helpers::build_jit_helpers();
        for (path, module_proto) in graph_build.modules.iter() {
            if path == &graph_build.entry_path {
                continue;
            }
            if debug.summary {
                eprintln!("\n=== MODULE: {} ===", path);
                varn_debug::summary::debug_summary(module_proto);
            }
            // These two print their own header only when they have content, so
            // a filtered run does not emit a banner per silent module.
            if debug.tiers {
                varn_debug::tiers::debug_tiers(module_proto, debug, &helpers, Some(path));
            }
            if debug.bails {
                varn_debug::tiers::debug_bails(module_proto, debug, &helpers, Some(path));
            }
        }
    }

    let mut precompiled_map: FxHashMap<varn_core::ModuleId, Rc<FunctionProto>> =
        FxHashMap::default();
    for (path, module_proto) in graph_build.modules.iter() {
        if path != &graph_build.entry_path {
            precompiled_map.insert(
                varn_core::ModuleId::from_canonical_str(path),
                Rc::new(module_proto.clone()),
            );
        }
    }

    let graph_hash = graph_build.source_hashes.values().fold(0u64, |acc, &h| {
        acc.wrapping_mul(0x9e3779b97f4a7c15).wrapping_add(h)
    });
    let graph_artifact = ModuleGraphArtifact {
        format_version: CACHE_FORMAT_VERSION,
        entry_path: graph_build.entry_path.clone(),
        graph_hash,
        source_hashes: graph_build.source_hashes,
        modules: graph_build.modules,
        package_nodes: graph_build.package_nodes,
    };

    Ok(CompileOutput {
        entry_proto: proto,
        precompiled: Rc::new(precompiled_map),
        graph_artifact,
    })
}

fn print_module_graph(build: &crate::module_precompile::ModuleGraphBuild) {
    use varn_debug::colors::{BOLD, C_MODULES, R};
    println!("\n{BOLD}Module Dependency Graph{R}");
    println!("  Entry: {C_MODULES}{}{R}", shorten_path(&build.entry_path));
    println!();
    print_graph_node(
        &build.entry_path,
        &build.deps,
        &mut std::collections::HashSet::new(),
        "",
        true,
    );
    println!();
    println!("  {} modules total", build.deps.len());
}

fn print_graph_node(
    node: &str,
    deps: &std::collections::HashMap<String, Vec<String>>,
    visited: &mut std::collections::HashSet<String>,
    prefix: &str,
    is_last: bool,
) {
    use varn_debug::colors::{C_ERRORS, C_MODULES, R};
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
