use super::check::CheckResult;
use crate::error::CliError;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_compiler::FunctionProto;
use varn_core::ast::Program;
use varn_debug::flags::DebugFlags;
use varn_types::ModuleGraphArtifact;

pub const CACHE_FORMAT_VERSION: u32 = 8;

type PipelineResult<T> = Result<T, CliError>;

pub struct CompileOutput {
    pub entry_proto: FunctionProto,
    pub precompiled: Rc<FxHashMap<String, Rc<FunctionProto>>>,
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
        eprintln!("[Varn] generating bytecode...");
    }

    let exports = varn_checker::module_resolver::resolve_module_exports_ref(&program.filename, &mut vec![]);
    let mut export_names: Vec<std::rc::Rc<str>> = exports.keys().map(|k| std::rc::Rc::from(k.as_str())).collect();
    export_names.sort();

    let proto = varn_compiler::compile(
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
        eprintln!("[Varn] resolving module graph...");
    }
    let graph_build =
        crate::module_precompile::build_module_graph(program, source, &program.filename, &proto)
            .map_err(|e| CliError::fatal(format!("module graph error: {e}")))?;

    let mut precompiled_map = FxHashMap::default();
    for (path, module_proto) in graph_build.modules.iter() {
        if path != &graph_build.entry_path {
            let clean_path = path
                .strip_prefix(varn_modules::EMBEDDED_MODULE_PREFIX)
                .unwrap_or(path);
            precompiled_map.insert(clean_path.to_owned(), Rc::new(module_proto.clone()));
        }
    }

    // Fibonacci mix: order-sensitive, avoids XOR-fold collisions from permuted modules.
    let graph_hash = graph_build
        .source_hashes
        .values()
        .fold(0u64, |acc, &h| {
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
