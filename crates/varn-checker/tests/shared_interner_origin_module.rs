#![allow(unused_crate_dependencies)]






















use std::fs;
use std::path::PathBuf;
use varn_checker::module_resolver::{DiskResolver, ImportResolver};




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

    fs::write(&lib_path, "export function greet(): int {\n  return 1\n}\n").expect("write lib.vn");
    
    
    
    
    
    
    
    
    
    
    
    
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

    
    
    
    
    
    
    let resolved_origin = bind.interner.resolve(origin_atom);

    let expected = varn_modules::canonical_or_original(&lib_path);
    assert_eq!(
        resolved_origin, expected,
        "origin_module must resolve to lib.vn's real path, not garbage or an unrelated string"
    );

    let _ = fs::remove_dir_all(&dir);
}
