//! Task 7e regression test: one `AtomInterner` per compilation, not one per
//! file.
//!
//! Before this fix, `varn_parser::parse` minted a fresh `AtomInterner` on
//! every call, so `DiskResolver::parse_and_cache` (imports) and the pipeline
//! entry-file parse (`varn-pipeline/src/parse.rs`) each produced `Atom`s from
//! *different* tables. `Symbol::origin_module: Atom` — set in
//! `binder/imports.rs` by interning the imported module's path in the
//! *importer's own* `BindResult::interner` — was then resolved with
//! `bind.interner.resolve(atom)` against whichever `BindResult` the caller
//! happened to hold. Two different sessions' `Atom(3)` never meant the same
//! text, so this either resolved to an unrelated string or panicked on an
//! out-of-bounds index.
//!
//! `DiskResolver` (the thread-local compilation singleton every `.vn` compile
//! already funnels through) now owns the one `AtomInterner` for the whole
//! run, and every parse it drives seeds from — and publishes back into — that
//! same table. This test binds two real files on disk, one importing a value
//! from the other, and confirms the imported symbol's `origin_module` atom
//! resolves to the *correct* absolute path through the *importing* module's
//! own `BindResult::interner` — the exact cross-module read that was broken.

use std::fs;
use std::path::PathBuf;
use varn_checker::module_resolver::{DiskResolver, ImportResolver};

/// A directory under `std::env::temp_dir()` unique to this test process, so
/// concurrent `cargo test` runs (and repeated runs) never collide on the same
/// `.vn` paths.
fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "varn-task7e-{tag}-{}-{}",
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
fn imported_symbol_origin_module_resolves_to_the_real_module_path() {
    let dir = scratch_dir("origin-module");

    let lib_path = dir.join("lib.vn");
    let main_path = dir.join("main.vn");

    fs::write(&lib_path, "export function greet(): int {\n  return 1\n}\n")
        .expect("write lib.vn");
    // A namespace import (`* as lib`), not a named import: `binder/imports.rs`
    // sets a namespace symbol's `origin_module` straight from
    // `self.interner.intern(module_path)` on the *importer's* own binder — no
    // detour through `module_resolver::exports::atom_or_placeholder`, which
    // carries a separate, already-documented resolution gap (it can only
    // `.get()` a `BindResult`'s interner, never `.intern()` new text into the
    // shared table, so an absolute path that was never itself a source lexeme
    // silently falls back to `Atom::default()`). That gap predates this task,
    // is out of its scope, and would otherwise make this assertion flaky on
    // an unrelated bug. The namespace path isolates exactly what Task 7e
    // fixed: one `Atom` table shared by the importer's parse and the
    // resolver's module cache.
    fs::write(
        &main_path,
        "import * as lib from \"./lib.vn\"\nlet x: int = lib.greet()\n",
    )
    .expect("write main.vn");

    let resolver = DiskResolver::new();
    let main_str = main_path.to_string_lossy().into_owned();
    let bind = resolver
        .module_bind(&main_str)
        .expect("main.vn must bind — parse/import wiring is what this test exercises");

    assert!(
        !bind.diagnostics.has_errors(),
        "main.vn should bind and check cleanly: {:?}",
        bind.diagnostics.errors().collect::<Vec<_>>()
    );

    let lib_sym = bind
        .global_symbols()
        .find(|s| bind.interner.resolve(s.name) == "lib")
        .expect("`lib` must be bound as a global namespace symbol in main.vn");

    let origin_atom = lib_sym
        .origin_module
        .expect("a namespace import must carry origin_module");

    // The bug this task fixes: resolving `origin_atom` through *this*
    // module's own interner clone used to be meaningless, because the atom
    // that `binder/imports.rs` interned while binding `main.vn` and the atom
    // space `lib.vn` was parsed under were two unrelated tables. With one
    // shared `AtomInterner` per compilation, the same index means the same
    // text everywhere.
    let resolved_origin = bind.interner.resolve(origin_atom);

    let expected = varn_modules::canonical_or_original(&lib_path);
    assert_eq!(
        resolved_origin, expected,
        "origin_module must resolve to lib.vn's real path, not garbage or an unrelated string"
    );

    let _ = fs::remove_dir_all(&dir);
}
