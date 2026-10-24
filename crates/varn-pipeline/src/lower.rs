use crate::Failure;
use varn_compiler::FunctionProto;
use varn_core::ast::{AstArena, Program};
use varn_sem::resolver::ImportResolver;

pub(crate) fn sorted_export_names(exports: &varn_sem::exports::ExportMap) -> Vec<std::sync::Arc<str>> {
    let mut names: Vec<std::sync::Arc<str>> = exports
        .keys()
        .map(|k| std::sync::Arc::from(k.as_str()))
        .collect();
    names.sort();
    names
}

pub(crate) fn report_export_collisions(
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

pub(crate) fn emit_tir(
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

pub(crate) fn tir_import_sources(tir: &varn_tir::TirModule) -> Vec<std::sync::Arc<str>> {
    tir.imports
        .iter()
        .filter(|i| !i.is_type_only && !i.specs.is_empty())
        .map(|i| std::sync::Arc::from(i.source.as_ref()))
        .collect()
}

pub(crate) fn resolve_import_slots(
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

pub(crate) fn inject_star_reexports(
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

pub(crate) fn compile_tir(
    tir: &varn_tir::TirModule,
    export_names: Vec<std::sync::Arc<str>>,
    source: &str,
    measure: bool,
) -> (Result<FunctionProto, String>, std::time::Duration) {
    let (result, opt_time) =
        varn_compiler::from_tir::compile_module(tir, export_names, source, measure);
    (result.map_err(|e| format!("{e:?}")), opt_time)
}

pub(crate) struct LowerInput<'a> {
    pub(crate) program: &'a Program,
    pub(crate) arena: &'a AstArena,
    pub(crate) check: &'a varn_sem::output::CheckResult,
    pub(crate) resolver: &'a crate::ModuleResolver,
    pub(crate) source: &'a str,
    pub(crate) measure: bool,
}

pub(crate) struct Lowered {
    pub(crate) tir: varn_tir::TirModule,
    pub(crate) proto: FunctionProto,
    pub(crate) optimize: std::time::Duration,
}

pub(crate) fn lower_module(input: LowerInput<'_>) -> Result<Lowered, Failure> {
    let LowerInput {
        program,
        arena,
        check,
        resolver,
        source,
        measure,
    } = input;
    let path = program.filename.as_ref();

    let exports = if varn_core::ModuleKind::of(path).is_some() {
        resolver.stdlib_exports(path)
    } else {
        resolver.module_exports(path, &mut vec![])
    };
    report_export_collisions(&exports, path).map_err(Failure::internal)?;
    let export_names = sorted_export_names(&exports);

    let mut tir = emit_tir(program, arena, check);
    crate::layer_link::verify_synthetic_imports(
        path,
        program,
        arena,
        &check.bind.interner,
        tir_import_sources(&tir).iter().map(|s| s.as_ref()),
    )
    .map_err(Failure::internal)?;
    inject_star_reexports(&mut tir, &exports);
    resolve_import_slots(&mut tir, resolver, path);
    let (proto, optimize) = compile_tir(&tir, export_names, source, measure);
    let proto = proto.map_err(|e| {
        Failure::internal(format!(
            "{}: {e}",
            varn_core::term::chalk::chalk("error[emit:tir]")
                .red()
                .bold()
        ))
    })?;
    Ok(Lowered {
        tir,
        proto,
        optimize,
    })
}
