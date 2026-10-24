use crate::lower::{lower_module, LowerInput, Lowered};
use crate::{Executable, Failure};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};
use varn_compiler::FunctionProto;
use varn_core::ast::{AstArena, Program};
use varn_types::ModuleGraphArtifact;

pub(crate) struct Built {
    pub executable: Executable,
    pub lower: Duration,
    pub optimize: Duration,
    pub link: Duration,
}

pub(crate) fn build_executable(
    program: &Program,
    arena: &AstArena,
    source: &str,
    check: &varn_sem::output::CheckResult,
    resolver: &crate::ModuleResolver,
    measure: bool,
) -> Result<Built, Failure> {
    let lowering = Instant::now();
    let Lowered {
        tir,
        proto,
        optimize,
    } = lower_module(LowerInput {
        program,
        arena,
        check,
        resolver,
        source,
        measure,
    })?;
    let lower = lowering.elapsed().saturating_sub(optimize);

    let linking = Instant::now();
    let graph_build = crate::module_precompile::build_module_graph(
        program,
        arena,
        source,
        &program.filename,
        &proto,
        &check.bind.interner,
        resolver,
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

    let mut precompiled: FxHashMap<varn_core::ModuleId, Rc<FunctionProto>> = FxHashMap::default();
    for (path, module_proto) in graph_build.modules.iter() {
        if path != &graph_build.entry_path {
            precompiled.insert(
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

    Ok(Built {
        executable: Executable {
            entry_proto: proto,
            precompiled: Rc::new(precompiled),
            graph_artifact,
            deps: graph_build.deps,
            warnings: graph_build.warnings,
        },
        lower,
        optimize,
        link: linking.elapsed(),
    })
}
