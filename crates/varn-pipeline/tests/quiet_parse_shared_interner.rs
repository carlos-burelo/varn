//! Task 7e follow-up regression test: `quiet_parse::parse_module` (used by
//! `module_precompile::build_module_graph`, the real multi-module compile
//! path behind `vn run`/`vn build`) used to mint its own throwaway
//! `AtomInterner` per module instead of sharing the resolver's table.
//!
//! `build_module_graph` re-parses and re-checks every *imported* module
//! (`quiet_parse::parse_module` + `Checker::check_with`) to emit its own
//! bytecode, separately from the checker's own import resolution
//! (`DiskResolver::parse_and_cache`, already fixed under Task 7e and shared
//! through the whole compilation). When a re-parsed module itself imports a
//! *named* symbol from a third module, `binder/imports.rs` does:
//!
//!   s.origin_module = resolved.origin_module.or(module_path_atom);
//!   ...
//!   let origin_rc: Arc<str> = Arc::from(self.interner.resolve(*origin));
//!
//! `resolved.origin_module` is an `Atom` mint by the *exporting* module's own
//! bind (reached through the resolver's shared, growing table), while
//! `self.interner` here is the *importing* module's own binder interner —
//! before this fix, `quiet_parse::parse_module`'s isolated, freshly-created
//! table. Resolving a shared-table index (already large, after everything the
//! resolver has interned so far) against a tiny isolated table indexes out of
//! bounds and panics inside `AtomInterner::resolve`.
//!
//! This test builds a three-module chain — entry -> mid -> leaf, where `mid`
//! imports a *named* export from `leaf` and both re-exports and calls it — so
//! `build_module_graph`'s re-check of `mid.vn` exercises exactly that path,
//! and confirms the whole graph compiles without panicking or erroring.

use std::fs;
use std::path::PathBuf;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "varn-task7e-fix1-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

#[test]
fn build_module_graph_resolves_named_reexport_origin_module_without_panicking() {
    varn_pipeline::resolver::reset();

    let dir = scratch_dir("named-reexport");

    let leaf_path = dir.join("leaf.vn");
    let mid_path = dir.join("mid.vn");
    let entry_path = dir.join("entry.vn");

    fs::write(
        &leaf_path,
        "export function leafValue(): int {\n  return 42\n}\n",
    )
    .expect("write leaf.vn");

    // A *named* import (not `* as`) of a symbol whose `origin_module` was
    // already stamped by `leaf.vn`'s own export map — this is what forces
    // `binder/imports.rs`'s `resolved.origin_module.or(module_path_atom)` and
    // the subsequent `self.interner.resolve(*origin)` to run against
    // `mid.vn`'s own (pre-fix: isolated) interner while re-checked inside
    // `build_module_graph`.
    fs::write(
        &mid_path,
        "import { leafValue } from \"./leaf.vn\"\nexport function midValue(): int {\n  return leafValue()\n}\n",
    )
    .expect("write mid.vn");

    fs::write(
        &entry_path,
        "import { midValue } from \"./mid.vn\"\nlet result: int = midValue()\n",
    )
    .expect("write entry.vn");

    let source = fs::read_to_string(&entry_path).expect("read entry.vn");
    let entry_str = entry_path.to_string_lossy().into_owned();

    let result = std::panic::catch_unwind(|| {
        varn_pipeline::compile_source_for_build(
            &source,
            &entry_str,
            false,
            &varn_pipeline::DebugFlags::default(),
        )
    });

    let _ = fs::remove_dir_all(&dir);

    match result {
        Err(_) => panic!(
            "build_module_graph panicked while re-checking a module with a named \
             re-exported import — this is the cross-interner Atom index bug \
             Task 7e's quiet_parse fix closes"
        ),
        Ok(Err(e)) => panic!("expected the module graph to compile cleanly, got: {e}"),
        Ok(Ok(_compiled)) => {}
    }
}
