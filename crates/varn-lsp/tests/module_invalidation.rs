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
