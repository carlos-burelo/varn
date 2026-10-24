use super::check::CheckResult;
use crate::Failure;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_compiler::FunctionProto;
use varn_core::ast::{AstArena, Program};
use varn_core::debug_flags::DebugFlags;
use crate::lower::{lower_module, LowerInput, Lowered};
use varn_types::ModuleGraphArtifact;

type PipelineResult<T> = Result<T, Failure>;

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
    session: &crate::ModuleResolver,
    sink: &dyn crate::debug_sink::DebugSink,
) -> PipelineResult<CompileOutput> {
    if verbose {
        varn_core::term::terminal::tagged("Varn", "generating bytecode...");
    }

    let Lowered { tir, proto } = lower_module(LowerInput {
        program,
        arena: ast_arena,
        check: &check_result.checker_result,
        resolver: session,
        source,
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
    .map_err(|e| match e {
        diagnostics @ Failure::Diagnostics(_) => diagnostics,
        other => Failure::internal(format!("module graph error: {other}")),
    })?;

    let mut class_tables = graph_build.class_tables.clone();
    class_tables.insert(
        program.filename.to_string(),
        crate::class_link::ModuleClasses {
            classes: tir.classes.clone(),
            origins: tir
                .class_origins
                .iter()
                .map(|o| o.as_ref().map(|s| s.to_string()))
                .collect(),
            types: tir.types.clone(),
        },
    );
    crate::class_link::verify_foreign_layouts(&class_tables).map_err(Failure::internal)?;

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

    let mut hash_keys: Vec<&String> = graph_build.source_hashes.keys().collect();
    hash_keys.sort();
    let graph_hash = hash_keys.iter().fold(0u64, |acc, k| {
        let h = graph_build.source_hashes[*k];
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
