use crate::Failure;
use vn_compiler::FunctionProto;
use vn_core::ast::{AstArena, Program};
use vn_sem::resolver::ImportResolver;

pub(crate) fn sorted_export_names(
    exports: &vn_sem::exports::ExportMap,
) -> Vec<std::sync::Arc<str>> {
    let mut names: Vec<std::sync::Arc<str>> = exports
        .keys()
        .map(|k| std::sync::Arc::from(k.as_str()))
        .collect();
    names.sort();
    names
}

pub(crate) fn report_export_collisions(
    exports: &vn_sem::exports::ExportMap,
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
    check: &vn_sem::output::CheckResult,
) -> vn_tir::TirModule {
    vn_emit::emit_module(
        program,
        ast_arena,
        &check.bind,
        &check.expr_table,
        &check.call_mappings,
        &check.desugar,
    )
}

pub(crate) fn written_import_sources(tir: &vn_tir::TirModule) -> Vec<std::sync::Arc<str>> {
    tir.imports
        .iter()
        .filter(|i| {
            !i.is_type_only
                && !i.specs.is_empty()
                && !i
                    .specs
                    .iter()
                    .all(|spec| vn_core::extension_names::is_extension_name(&spec.local))
        })
        .map(|i| std::sync::Arc::from(i.source.as_ref()))
        .collect()
}

pub(crate) fn resolve_import_slots(
    tir: &mut vn_tir::TirModule,
    resolver: &dyn vn_sem::resolver::ImportResolver,
    module_path: &str,
) {
    use vn_core::ImportSpecifier as IS;
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
                vn_tir::TirImportKind::Named(n) => n.clone(),
                vn_tir::TirImportKind::Default => std::sync::Arc::from("default"),
                vn_tir::TirImportKind::Namespace => continue,
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
    tir: &mut vn_tir::TirModule,
    exports: &vn_sem::exports::ExportMap,
) {
    use std::collections::BTreeSet;
    let covered: BTreeSet<&str> = tir.exports.iter().map(|e| e.exported.as_ref()).collect();
    let mut missing: Vec<(&String, &vn_sem::symbol::Symbol)> = exports
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
        tir.exports.push(vn_tir::TirExport {
            exported: std::sync::Arc::from(name.as_str()),
            local: std::sync::Arc::from(declared),
            reexport_from: Some(std::sync::Arc::from(src)),
            namespace: false,
        });
    }
}

pub(crate) fn compile_tir(
    tir: &vn_tir::TirModule,
    export_names: Vec<std::sync::Arc<str>>,
    source: &str,
    measure: bool,
) -> (Result<FunctionProto, String>, std::time::Duration) {
    let (result, opt_time) =
        vn_compiler::from_tir::compile_module(tir, export_names, source, measure);
    (result.map_err(|e| e.to_string()), opt_time)
}

#[derive(Default)]
pub(crate) struct TemplateLibrary {
    modules: rustc_hash::FxHashMap<String, std::sync::Arc<vn_tir::package::ModuleTemplates>>,
    shipped: std::sync::Mutex<
        rustc_hash::FxHashMap<String, Option<std::sync::Arc<vn_tir::package::ModuleTemplates>>>,
    >,
}

impl TemplateLibrary {
    fn shipped(&self, specifier: &str) -> Option<std::sync::Arc<vn_tir::package::ModuleTemplates>> {
        let mut cache = self.shipped.lock().ok()?;
        if let Some(known) = cache.get(specifier) {
            return known.clone();
        }
        let decoded = vn_modules::provider::get()
            .and_then(|provider| provider.module(specifier))
            .and_then(|module| module.templates)
            .and_then(|bytes| postcard::from_bytes::<vn_tir::package::ModuleTemplates>(bytes).ok())
            .map(std::sync::Arc::new);
        cache.insert(specifier.to_owned(), decoded.clone());
        decoded
    }

    pub(crate) fn insert(&mut self, path: &str, templates: vn_tir::package::ModuleTemplates) {
        self.modules.insert(
            vn_modules::canonical_or_original(std::path::Path::new(path)),
            std::sync::Arc::new(templates),
        );
    }
}

struct LibraryView<'a> {
    library: &'a TemplateLibrary,
    resolver: &'a crate::ModuleResolver,
    module_path: &'a str,
}

impl vn_tir::link::TemplateSource for LibraryView<'_> {
    fn templates(
        &self,
        import_source: &str,
    ) -> Option<std::sync::Arc<vn_tir::package::ModuleTemplates>> {
        use vn_core::ImportSpecifier as IS;
        match IS::parse(import_source) {
            IS::Relative(_) => {}
            IS::Stdlib(_) | IS::Core(_) => return self.library.shipped(import_source),
            IS::Runtime(_) | IS::Package(_) => return None,
        }
        let base = std::path::Path::new(self.module_path)
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let resolved = self.resolver.resolve_specifier(base, import_source)?;
        let key = vn_modules::canonical_or_original(std::path::Path::new(&resolved));
        self.library.modules.get(&key).cloned()
    }
}

pub(crate) struct LowerInput<'a> {
    pub(crate) program: &'a Program,
    pub(crate) arena: &'a AstArena,
    pub(crate) check: &'a vn_sem::output::CheckResult,
    pub(crate) resolver: &'a crate::ModuleResolver,
    pub(crate) source: &'a str,
    pub(crate) library: &'a TemplateLibrary,
    pub(crate) measure: bool,
}

pub(crate) struct Lowered {
    pub(crate) tir: vn_tir::TirModule,
    pub(crate) proto: FunctionProto,
    pub(crate) templates: vn_tir::package::ModuleTemplates,
    pub(crate) optimize: std::time::Duration,
}

fn exports_of(
    program: &Program,
    resolver: &crate::ModuleResolver,
) -> std::sync::Arc<vn_sem::exports::ExportMap> {
    let path = program.filename.as_ref();
    if vn_core::ModuleKind::of(path).is_some() {
        resolver.stdlib_exports(path)
    } else {
        resolver.module_exports(path, &mut vec![])
    }
}

pub(crate) fn check_exports(
    program: &Program,
    resolver: &crate::ModuleResolver,
) -> Result<(), Failure> {
    let exports = exports_of(program, resolver);
    report_export_collisions(&exports, program.filename.as_ref()).map_err(Failure::internal)
}

pub(crate) fn lower_module(input: LowerInput<'_>) -> Result<Lowered, Failure> {
    let LowerInput {
        program,
        arena,
        check,
        resolver,
        source,
        library,
        measure,
    } = input;
    let path = program.filename.as_ref();

    let exports = exports_of(program, resolver);
    report_export_collisions(&exports, path).map_err(Failure::internal)?;
    let export_names = sorted_export_names(&exports);

    let mut tir = emit_tir(program, arena, check);
    crate::layer_link::verify_synthetic_imports(
        path,
        program,
        arena,
        &check.bind.interner,
        written_import_sources(&tir).iter().map(|s| s.as_ref()),
    )
    .map_err(Failure::internal)?;
    inject_star_reexports(&mut tir, &exports);
    resolve_import_slots(&mut tir, resolver, path);
    vn_tir::link::link(
        &mut tir,
        &LibraryView {
            library,
            resolver,
            module_path: path,
        },
    );
    let templates = vn_tir::package::extract(&tir);
    let (proto, optimize) = compile_tir(&tir, export_names, source, measure);
    let proto = proto.map_err(|e| {
        Failure::internal(format!(
            "{}: {e}",
            vn_core::term::chalk::chalk("error[emit:tir]").red().bold()
        ))
    })?;
    Ok(Lowered {
        tir,
        proto,
        templates,
        optimize,
    })
}
