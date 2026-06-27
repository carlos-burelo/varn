use super::check::CheckResult;
use crate::error::CliError;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_opt::FunctionProto;
use varn_core::ast::Program;
use varn_debug::flags::DebugFlags;
use varn_types::ModuleGraphArtifact;
use varn_utilities::terminal;

// Kept in lockstep with `varn_modules::artifact::COMPILER_CACHE_VERSION`; bump
// on any bytecode/codegen change. 11: closure self-reference lowering fix.
pub const CACHE_FORMAT_VERSION: u32 = 11;

type PipelineResult<T> = Result<T, CliError>;

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
        terminal::tagged("Varn", "generating bytecode...");
    }

    let exports =
        varn_checker::module_resolver::resolve_module_exports_ref(&program.filename, &mut vec![]);
    let mut export_names: Vec<std::rc::Rc<str>> = exports
        .keys()
        .map(|k| std::rc::Rc::from(k.as_str()))
        .collect();
    export_names.sort();

    let proto = varn_opt::compile_module(
        program,
        &check_result.checker_result.type_annotations,
        &check_result.checker_result.extension_calls,
        &check_result.checker_result.extension_members,
        &check_result.checker_result.extension_set_members,
        export_names,
    )
    .map_err(|e| {
        CliError::fatal(format!(
            "{}{}error[emit]{}: {}",
            varn_debug::colors::BOLD,
            varn_debug::colors::C_ERRORS,
            varn_debug::colors::R,
            e
        ))
    })?;

    if debug.bytecode {
        varn_debug::bytecode::debug_bytecode(&proto, debug);
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
        terminal::tagged("Varn", "resolving module graph...");
    }
    let graph_build =
        varn_pipeline::module_precompile::build_module_graph(program, source, &program.filename, &proto)
            .map_err(|e| CliError::fatal(format!("module graph error: {e}")))?;

    if debug.graph {
        print_module_graph(&graph_build);
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

fn print_module_graph(build: &varn_pipeline::module_precompile::ModuleGraphBuild) {
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
