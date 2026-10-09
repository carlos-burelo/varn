use rustc_hash::FxHashSet as HashSet;
use varn_core::ast::{AstArena, Program};
use varn_core::AtomInterner;

pub fn verify_synthetic_imports(
    module_path: &str,
    program: &Program,
    ast_arena: &AstArena,
    interner: &AtomInterner,
    tir_sources: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<(), String> {
    let ast_sources: HashSet<String> =
        crate::import_collector::collect_imports(program, ast_arena, interner);
    for src in tir_sources {
        let src = src.as_ref();
        if ast_sources.contains(src) {
            continue;
        }
        if varn_modules::is_prelude_module(src) {
            continue;
        }
        return Err(format!(
            "E_LAYER: synthetic import '{src}' in '{module_path}' is not a prelude module"
        ));
    }
    Ok(())
}
