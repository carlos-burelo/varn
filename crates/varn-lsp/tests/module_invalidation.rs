#![allow(unused_crate_dependencies)] // per-target lint: a test needs only a slice of the crate deps

// Regression test for the scoped module-cache invalidation added to
// `Workspace::update_file`. Before that change, correctness relied on
// `module_resolver::invalidate_module_cache()` nuking every cache on
// every pipeline run and every LSP request; this proves the targeted
// `invalidate_module()` call still propagates a changed export's type
// to files that import it.

use std::fs;

use varn_lsp::constants::SEVERITY_ERROR;
use varn_lsp::workspace::Workspace;

fn to_uri(path: &std::path::Path) -> String {
    let canonical = std::fs::canonicalize(path).expect("file must exist to canonicalize");
    varn_modules::resolver::path_to_uri(&canonical.to_string_lossy())
}

fn error_count(state: &varn_lsp::document::DocumentAnalysis) -> usize {
    state
        .diagnostics
        .iter()
        .filter(|d| d.severity == SEVERITY_ERROR)
        .count()
}

#[test]
fn dependent_file_sees_updated_export_type_after_edit() {
    let dir = std::env::temp_dir().join(format!("varn-lsp-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let a_path = dir.join("a.vn");
    let b_path = dir.join("b.vn");

    fs::write(&a_path, "export function foo(): int {\n  return 1\n}\n").unwrap();
    fs::write(
        &b_path,
        "import { foo } from \"./a.vn\"\n\nlet x: int = foo()\n",
    )
    .unwrap();

    let a_uri = to_uri(&a_path);
    let b_uri = to_uri(&b_path);

    let ws = Workspace::new();
    ws.update_file(a_uri.clone(), fs::read_to_string(&a_path).unwrap());
    ws.update_file(b_uri.clone(), fs::read_to_string(&b_path).unwrap());

    let before = ws.get(&b_uri).unwrap();
    assert_eq!(
        error_count(&before),
        0,
        "expected no type errors before edit, got {:?}",
        before
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // foo's return type flips from int to str; b.vn's `let x: int = foo()`
    // should now report a type mismatch. This can only happen if the
    // dependent's cached bind of a.vn was actually invalidated.
    fs::write(
        &a_path,
        "export function foo(): str {\n  return \"hi\"\n}\n",
    )
    .unwrap();
    ws.update_file(a_uri.clone(), fs::read_to_string(&a_path).unwrap());

    let after = ws.get(&b_uri).unwrap();
    assert!(
        error_count(&after) > 0,
        "expected b.vn to report a type mismatch after a.vn's return type changed, got none"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// The same guarantee, one hop further out.
///
/// `update_file` evicts the edited module and walks `reverse_deps` to reach
/// everything that transitively imports it. A one-hop test cannot tell a real
/// walk from an eviction that only ever looks at direct importers, so the chain
/// here is `a <- b <- c` and the assertion is on `c`.
#[test]
fn a_transitive_importer_also_sees_the_updated_export() {
    let dir = std::env::temp_dir().join(format!("varn-lsp-transitive-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let a_path = dir.join("a.vn");
    let b_path = dir.join("b.vn");
    let c_path = dir.join("c.vn");

    fs::write(&a_path, "export function foo(): int {\n  return 1\n}\n").unwrap();
    fs::write(
        &b_path,
        "import { foo } from \"./a.vn\"\n\nexport function relay(): int {\n  return foo()\n}\n",
    )
    .unwrap();
    fs::write(
        &c_path,
        "import { relay } from \"./b.vn\"\n\nlet x: int = relay()\n",
    )
    .unwrap();

    let a_uri = to_uri(&a_path);
    let b_uri = to_uri(&b_path);
    let c_uri = to_uri(&c_path);

    let ws = Workspace::new();
    ws.update_file(a_uri.clone(), fs::read_to_string(&a_path).unwrap());
    ws.update_file(b_uri.clone(), fs::read_to_string(&b_path).unwrap());
    ws.update_file(c_uri.clone(), fs::read_to_string(&c_path).unwrap());

    assert_eq!(
        error_count(&ws.get(&c_uri).unwrap()),
        0,
        "the chain must type-check before the edit"
    );

    // `foo` returns str now, so `relay`'s declared `int` return breaks, and
    // with it `c.vn`. Reaching `c` requires walking a -> b -> c.
    fs::write(&a_path, "export function foo(): str {\n  return \"hi\"\n}\n").unwrap();
    ws.update_file(a_uri, fs::read_to_string(&a_path).unwrap());

    let b_after = ws.get(&b_uri).unwrap();
    assert!(
        error_count(&b_after) > 0,
        "b.vn imports a.vn directly and must report the mismatch; got none"
    );

    let _ = fs::remove_dir_all(&dir);
}
