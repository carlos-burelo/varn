#![allow(unused_crate_dependencies)]

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

    let resolver1 = DiskResolver::new();
    let bind1 = resolver1
        .module_bind(&lib_str)
        .expect("greeter.vn must bind");
    assert!(
        !bind1.diagnostics.has_errors(),
        "greeter.vn should bind cleanly: {:?}",
        bind1.diagnostics.errors().collect::<Vec<_>>()
    );

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

    let atom = bind2.interner.get("makeGreeting").expect(
        "the reloaded interner must have interned 'makeGreeting' while re-interning symbols",
    );
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
