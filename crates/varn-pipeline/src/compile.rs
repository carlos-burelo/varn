use super::check::CheckResult;
use crate::Failure;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_compiler::FunctionProto;
use varn_core::ast::{AstArena, Program};
use varn_core::debug_flags::DebugFlags;
use varn_sem::resolver::ImportResolver;
use varn_types::ModuleGraphArtifact;

type PipelineResult<T> = Result<T, Failure>;

pub fn sorted_export_names(exports: &varn_sem::exports::ExportMap) -> Vec<std::sync::Arc<str>> {
    let mut names: Vec<std::sync::Arc<str>> = exports
        .keys()
        .map(|k| std::sync::Arc::from(k.as_str()))
        .collect();
    names.sort();
    names
}

pub fn report_export_collisions(
    exports: &varn_sem::exports::ExportMap,
    module_path: &str,
) -> Result<(), String> {
    if exports.collisions.is_empty() {
        return Ok(());
    }
    let mut collisions: Vec<String> = exports
        .collisions
        .iter()
        .map(|c| {
            format!(
                "'{}' kept from '{}', dropped from '{}'",
                c.name, c.kept_from, c.dropped_from
            )
        })
        .collect();
    collisions.sort();
    collisions.dedup();
    Err(format!(
        "E_EXPORT_COLLISION in '{module_path}': {}; disambiguate with explicit named re-exports",
        collisions.join("; ")
    ))
}

pub fn emit_tir(
    program: &Program,
    ast_arena: &AstArena,
    check: &varn_sem::output::CheckResult,
) -> varn_tir::TirModule {
    varn_emit::emit_module(
        program,
        ast_arena,
        &check.bind,
        &check.expr_table,
        &check.call_mappings,
        &check.desugar,
    )
}

pub fn tir_import_sources(tir: &varn_tir::TirModule) -> Vec<std::sync::Arc<str>> {
    tir.imports
        .iter()
        .filter(|i| !i.is_type_only && !i.specs.is_empty())
        .map(|i| std::sync::Arc::from(i.source.as_ref()))
        .collect()
}

pub fn resolve_import_slots(
    tir: &mut varn_tir::TirModule,
    resolver: &dyn varn_sem::resolver::ImportResolver,
    module_path: &str,
) {
    use varn_core::ImportSpecifier as IS;
    for imp in tir.imports.iter_mut().filter(|i| !i.is_type_only) {
        let exports = match IS::parse(imp.source.as_ref()) {
            IS::Relative(_) => {
                let base = std::path::Path::new(module_path)
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."));
                let resolved = resolver
                    .resolve_specifier(base, imp.source.as_ref())
                    .unwrap_or_default();
                if resolved.is_empty() {
                    continue;
                }
                resolver.module_exports(&resolved, &mut vec![])
            }
            IS::Package(_) => continue,
            IS::Stdlib(_) | IS::Core(_) | IS::Runtime(_) => {
                resolver.stdlib_exports(imp.source.as_ref())
            }
        };
        for spec in imp.specs.iter_mut() {
            let name = match &spec.kind {
                varn_tir::TirImportKind::Named(n) => n.clone(),
                varn_tir::TirImportKind::Default => std::sync::Arc::from("default"),
                varn_tir::TirImportKind::Namespace => continue,
            };
            if let Some(sym) = exports.get(name.as_ref())
                && let Some(slot) = sym.slot_idx
                && slot <= u16::MAX as usize
            {
                spec.slot = Some(slot as u16);
            }
        }
    }
}

pub fn inject_star_reexports(
    tir: &mut varn_tir::TirModule,
    exports: &varn_sem::exports::ExportMap,
) {
    use std::collections::BTreeSet;
    let covered: BTreeSet<&str> = tir.exports.iter().map(|e| e.exported.as_ref()).collect();
    let mut missing: Vec<(&String, &varn_sem::symbol::Symbol)> = exports
        .iter()
        .filter(|(k, _)| !k.contains('.') && !covered.contains(k.as_str()))
        .collect();
    missing.sort_by(|a, b| a.0.cmp(b.0));
    for (name, sym) in missing {
        let Some(src) = sym
            .origin_module
            .and_then(|a| exports.table.name(a).map(str::to_owned))
        else {
            continue;
        };
        let declared = sym
            .original_name
            .filter(|_| name.contains("::"))
            .and_then(|a| exports.table.name(a))
            .unwrap_or(name.as_str());
        tir.exports.push(varn_tir::TirExport {
            exported: std::sync::Arc::from(name.as_str()),
            local: std::sync::Arc::from(declared),
            reexport_from: Some(std::sync::Arc::from(src)),
            namespace: false,
        });
    }
}

pub fn compile_tir(
    tir: &varn_tir::TirModule,
    export_names: Vec<std::sync::Arc<str>>,
    source: &str,
    measure: bool,
) -> (Result<FunctionProto, String>, std::time::Duration) {
    let (result, opt_time) =
        varn_compiler::from_tir::compile_module(tir, export_names, source, measure);
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
    session: &crate::ModuleResolver,
    sink: &dyn crate::debug_sink::DebugSink,
) -> PipelineResult<CompileOutput> {
    if verbose {
        varn_core::term::terminal::tagged("Varn", "generating bytecode...");
    }

    let exports = session.module_exports(&program.filename, &mut vec![]);
    crate::compile::report_export_collisions(&exports, &program.filename)
        .map_err(Failure::internal)?;
    let export_names = sorted_export_names(&exports);

    let mut tir = emit_tir(program, ast_arena, &check_result.checker_result);
    crate::layer_link::verify_synthetic_imports(
        &program.filename,
        program,
        ast_arena,
        &check_result.checker_result.bind.interner,
        tir_import_sources(&tir).iter().map(|s| s.as_ref()),
    )
    .map_err(Failure::internal)?;
    inject_star_reexports(&mut tir, &exports);
    resolve_import_slots(&mut tir, session, &program.filename);
    let (proto_result, _) = compile_tir(&tir, export_names, source, false);
    let proto = proto_result.map_err(|e| {
        Failure::internal(format!(
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
