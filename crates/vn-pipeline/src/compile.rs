use crate::executable::precompiled_modules;
use crate::lower::{lower_module, LowerInput, Lowered};
use crate::{Executable, Failure};
use std::time::{Duration, Instant};
use vn_core::ast::{AstArena, Program};
use vn_types::{ModuleGraphArtifact, TestEntry};

pub(crate) struct Built {
    pub executable: Executable,
    pub warnings: Vec<crate::DiagnosticReport>,
    pub lower: Duration,
    pub optimize: Duration,
    pub link: Duration,
}

fn test_entries(
    check: &vn_sem::output::CheckResult,
    tir: &vn_tir::TirModule,
) -> Result<Vec<TestEntry>, Failure> {
    check
        .test_targets
        .iter()
        .map(|target| {
            let global = tir
                .global_names
                .iter()
                .position(|name| name == &target.name)
                .ok_or_else(|| {
                    Failure::internal(format!(
                        "test function '{}' has no module global",
                        target.name
                    ))
                })?;
            Ok(TestEntry {
                name: target.name.to_string(),
                is_async: target.is_async,
                global: global as u32,
            })
        })
        .collect()
}

pub(crate) fn build_executable(
    program: &Program,
    arena: &AstArena,
    source: &str,
    check: &vn_sem::output::CheckResult,
    resolver: &crate::ModuleResolver,
    measure: bool,
) -> Result<Built, Failure> {
    let lowering = Instant::now();
    let found = crate::module_precompile::discover(
        program,
        arena,
        source,
        &program.filename,
        &check.bind.interner,
    )?;
    let dependencies = crate::module_precompile::lower_dependencies(&found, resolver)?;
    let Lowered {
        tir,
        proto,
        optimize,
        ..
    } = lower_module(LowerInput {
        program,
        arena,
        check,
        resolver,
        source,
        library: &dependencies.library,
        measure,
    })?;
    let lower = lowering.elapsed().saturating_sub(optimize);

    let linking = Instant::now();
    let graph_build = crate::module_precompile::assemble_graph(found, dependencies, &proto);

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

    let mut hash_keys: Vec<&String> = graph_build.source_hashes.keys().collect();
    hash_keys.sort();
    let graph_hash = hash_keys.iter().fold(0u64, |acc, k| {
        let h = graph_build.source_hashes[*k];
        acc.wrapping_mul(0x9e3779b97f4a7c15).wrapping_add(h)
    });
    let tests = test_entries(check, &tir)?;
    let graph_artifact = ModuleGraphArtifact {
        entry_path: graph_build.entry_path.clone(),
        graph_hash,
        source_hashes: graph_build.source_hashes,
        modules: graph_build.modules,
        package_nodes: graph_build.package_nodes,
        tests,
    };

    Ok(Built {
        executable: Executable {
            entry_proto: proto,
            precompiled: precompiled_modules(&graph_artifact),
            tests: graph_artifact.tests.clone(),
            graph_artifact,
            deps: graph_build.deps,
        },
        warnings: graph_build.warnings,
        lower,
        optimize,
        link: linking.elapsed(),
    })
}
