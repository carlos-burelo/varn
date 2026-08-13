#![allow(unused_crate_dependencies)] // per-target lint: a test needs only a slice of the crate deps

use varn_lsp::queries::exports::{ExportedSymbol, ModuleExports};
use varn_lsp::workspace::Workspace;

#[test]
fn test_query_firewall_detects_body_vs_signature_changes() {
    let sym1 = ExportedSymbol {
        name: "calculate".to_string(),
        kind_str: "Function".to_string(),
        signature_str: "fn(a: int, b: int): int".to_string(),
    };

    let exports_v1 = ModuleExports::build(vec![sym1.clone()]);
    let exports_v1_dup = ModuleExports::build(vec![sym1.clone()]);

    // Editing body does NOT change exported symbols -> Firewall stops propagation
    assert!(exports_v1.is_unchanged_from(&exports_v1_dup));

    // Changing signature -> Firewall detects change -> Propagates to dependents
    let sym2 = ExportedSymbol {
        name: "calculate".to_string(),
        kind_str: "Function".to_string(),
        signature_str: "fn(a: int, b: int, c: int): int".to_string(),
    };
    let exports_v2 = ModuleExports::build(vec![sym2]);

    assert!(!exports_v2.is_unchanged_from(&exports_v1));
}

#[test]
fn test_workspace_source_update_is_fast() {
    let ws = Workspace::new();
    let uri = "file:///test/main.vn";
    let source = "function main() { return 42; }";

    let (file_id, rev, token) = ws.update_source(uri, source);
    assert_eq!(file_id.0, 0);
    assert!(rev >= 1);
    assert!(!token.is_cancelled());

    // Updating source again bumps revision and cancels previous token
    let (_file_id2, rev2, token2) = ws.update_source(uri, "function main() { return 100; }");
    assert!(rev2 > rev);
    assert!(token.is_cancelled());
    assert!(!token2.is_cancelled());
}
