




use rustc_hash::FxHashSet;
use std::sync::Arc;
use varn_core::ast::{AstArena, ExprKind};
use varn_core::AtomInterner;
use varn_modules::layer::Layer;
use varn_tir::{TirImport, TirImportKind, TirImportSpec};



pub(super) fn prelude_imports(
    module: &str,
    declared: &FxHashSet<Arc<str>>,
    ast_arena: &AstArena,
    interner: &AtomInterner,
) -> Vec<TirImport> {
    if Layer::of_module(module) == Layer::Core {
        return Vec::new();
    }
    let used: FxHashSet<&str> = ast_arena
        .exprs()
        .filter_map(|e| match &e.kind {
            ExprKind::Identifier { name } => interner.try_resolve(*name),
            _ => None,
        })
        .collect();
    varn_modules::prelude_modules()
        .into_iter()
        .map(|spec| TirImport {
            source: Arc::from(spec.id),
            is_type_only: false,
            specs: spec
                .exports
                .iter()
                .filter(|name| used.contains(**name) && !declared.contains(**name))
                .map(|name| TirImportSpec {
                    local: Arc::from(*name),
                    kind: TirImportKind::Named(Arc::from(*name)),
                })
                .collect(),
        })
        .filter(|import| !import.specs.is_empty())
        .collect()
}
