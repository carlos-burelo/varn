//! Task 7d regression test: `Atom`s survive a round-trip through the
//! *on-disk* module-interface cache (`module_resolver/cache.rs`), across two
//! `DiskResolver`s standing in for two separate `vn` process runs.
//!
//! Each `DiskResolver` owns its own `AtomInterner`, starting empty (Task 7e).
//! Before this fix, `Symbol`'s `Atom`-typed fields (`name`, `origin_module`,
//! ...) were `#[serde(skip)]`, so a `Symbol` written to the `.vnm` cache by
//! one resolver and read back by a different one deserialized those fields as
//! `Atom(0)` — not an error, just silently resolved to whatever text sits at
//! index 0 of the *reading* resolver's interner. This test binds a module
//! with `resolver1` (which populates the on-disk cache as a side effect of
//! `module_bind`), then asks a brand-new `resolver2` — an empty interner,
//! like a fresh process — to bind the *same* source. Since the source hash is
//! unchanged, `resolver2` must satisfy the request from the cache
//! (`try_load_cache`), and every symbol name it hands back must still be the
//! real text, not garbage borrowed from `resolver2`'s own, unrelated atoms.
//!
//! It also exercises `CheckerScope::bindings` (also `Atom`-keyed and
//! `#[serde(skip)]`): resolving a name to a symbol id through the reloaded
//! scope chain must still work, which only holds if
//! `module_resolver::cache::rebuild_scope_bindings` rebuilt that map from the
//! re-interned arena.

use std::fs;
use std::path::PathBuf;
use varn_checker::module_resolver::{DiskResolver, ImportResolver};

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "varn-task7d-{tag}-{}-{}",
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
fn symbol_names_survive_disk_cache_across_fresh_resolvers() {
    let dir = scratch_dir("cache-roundtrip");
    let lib_path = dir.join("greeter.vn");
    fs::write(
        &lib_path,
        "export function makeGreeting(): int {\n  return 42\n}\nexport let farewell: int = 7\n",
    )
    .expect("write greeter.vn");
    let lib_str = lib_path.to_string_lossy().into_owned();

    // First "process": binds fresh (no cache yet), and — as a side effect of
    // `module_bind` — writes the interface to the on-disk cache.
    let resolver1 = DiskResolver::new();
    let bind1 = resolver1
        .module_bind(&lib_str)
        .expect("greeter.vn must bind");
    assert!(
        !bind1.diagnostics.has_errors(),
        "greeter.vn should bind cleanly: {:?}",
        bind1.diagnostics.errors().collect::<Vec<_>>()
    );

    // Second "process": a brand-new resolver, brand-new (empty) AtomInterner
    // — nothing in common with resolver1's atom table. Same source, same
    // hash, so this must be served from the cache file resolver1 just wrote.
    let resolver2 = DiskResolver::new();
    let bind2 = resolver2
        .module_bind(&lib_str)
        .expect("greeter.vn must bind from cache on a fresh resolver");

    let names: Vec<&str> = bind2
        .global_symbols()
        .map(|s| bind2.interner.resolve(s.name))
        .collect();
    assert!(
        names.contains(&"makeGreeting"),
        "cached reload must recover the real symbol name 'makeGreeting', got: {names:?}"
    );
    assert!(
        names.contains(&"farewell"),
        "cached reload must recover the real symbol name 'farewell', got: {names:?}"
    );

    // `CheckerScope::bindings` round-trip: resolving "makeGreeting" through
    // the reloaded global scope chain must find the reloaded symbol, which
    // only works if `rebuild_scope_bindings` restored the Atom-keyed lookup
    // table (it deserializes empty, being `#[serde(skip)]`).
    let atom = bind2
        .interner
        .get("makeGreeting")
        .expect("the reloaded interner must have interned 'makeGreeting' while re-interning symbols");
    let scope = bind2.scopes.get(bind2.global_scope);
    let resolved_id = scope
        .resolve(atom, &bind2.scopes)
        .expect("scope.resolve must find 'makeGreeting' after cache reload — CheckerScope::bindings must have been rebuilt");
    assert_eq!(
        bind2.interner.resolve(bind2.arena.get(resolved_id).name),
        "makeGreeting"
    );

    let _ = fs::remove_dir_all(&dir);
}
