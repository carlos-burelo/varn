use super::check::CheckResult;
use crate::PipelineError;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_checker::module_resolver::ImportResolver;
use varn_compiler::FunctionProto;
use varn_core::ast::{AstArena, Program};
use varn_debug_flags::DebugFlags;
use varn_types::ModuleGraphArtifact;

type PipelineResult<T> = Result<T, PipelineError>;

pub fn sorted_export_names(
    exports: &varn_checker::module_resolver::ExportMap,
) -> Vec<std::sync::Arc<str>> {
    let mut names: Vec<std::sync::Arc<str>> = exports
        .keys()
        .map(|k| std::sync::Arc::from(k.as_str()))
        .collect();
    names.sort();
    names
}

pub fn emit_and_compile(
    program: &Program,
    ast_arena: &AstArena,
    check: &varn_checker::CheckResult,
    export_names: Vec<std::sync::Arc<str>>,
    source: &str,
    measure: bool,
) -> (Result<FunctionProto, String>, std::time::Duration) {
    let tir = varn_checker::emit::emit_module(
        program,
        ast_arena,
        &check.bind,
        &check.expr_table,
        &check.call_mappings,
        &check.desugar,
    );
    let (result, opt_time) =
        varn_compiler::from_tir::compile_module(&tir, export_names, source, measure);
    (result.map_err(|e| format!("{e:?}")), opt_time)
}

pub struct CompileOutput {
    pub entry_proto: FunctionProto,
    pub precompiled: Rc<FxHashMap<varn_core::ModuleId, Rc<FunctionProto>>>,
    pub graph_artifact: ModuleGraphArtifact,
}

pub fn compile(
    program: &Program,
    ast_arena: &AstArena,
    source: &str,
    check_result: CheckResult,
    verbose: bool,
    debug: &DebugFlags,
    session: &crate::resolver::Session,
    sink: &dyn crate::debug_sink::DebugSink,
) -> PipelineResult<CompileOutput> {
    if verbose {
        varn_core::term::terminal::tagged("Varn", "generating bytecode...");
    }

    let exports = session
        .resolver()
        .module_exports(&program.filename, &mut vec![]);
    let export_names = sorted_export_names(&exports);

    let (proto_result, _) = emit_and_compile(
        program,
        ast_arena,
        &check_result.checker_result,
        export_names,
        source,
        false,
    );
    let proto = proto_result.map_err(|e| {
        PipelineError::fatal(format!(
            "{}: {e:?}",
            varn_core::term::chalk::chalk("error[emit:tir]")
                .red()
                .bold()
        ))
    })?;

    if verbose {
        varn_core::term::terminal::tagged("Varn", "resolving module graph...");
    }
    let graph_build = crate::module_precompile::build_module_graph(
        program,
        ast_arena,
        source,
        &program.filename,
        &proto,
        &check_result.checker_result.bind.interner,
        session,
    )
    .map_err(|e| PipelineError::fatal(format!("module graph error: {e}")))?;

    sink.compile(
        program,
        ast_arena,
        &check_result.checker_result,
        &proto,
        &graph_build,
        debug,
    );

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
