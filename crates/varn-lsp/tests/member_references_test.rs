#![allow(unused_crate_dependencies)]

use varn_lsp::features::references::build_references;
use varn_lsp::features::rename::build_rename;
use varn_lsp::workspace::Workspace;

#[test]
fn test_member_references_and_rename() {
    let source = r#"
class Account {
    balance: int;

    get_balance(): int {
        return this.balance;
    }

    deposit(amount: int) {
        this.balance = this.balance + amount;
    }
}

const acc = new Account();
acc.balance = 100;
const b = acc.balance;
"#;
    let uri = "file:///test/account.vn".to_string();
    let workspace = Workspace::new();
    workspace.update_file(uri.clone(), source.to_string());

    let doc = workspace.get(&uri).unwrap();

    // Find references for `balance` property (line 2, col 5)
    let refs = build_references(&doc, &workspace, 2, 5);
    assert!(
        refs.is_some(),
        "Should find references for property balance"
    );
    let locs = refs.unwrap();
    // balance appears at:
    // 1. declaration: line 2, balance
    // 2. this.balance: line 5
    // 3. this.balance = ...: line 9
    // 4. ... + amount: line 9
    // 5. acc.balance = 100: line 14
    // 6. acc.balance: line 15
    assert_eq!(
        locs.len(),
        6,
        "Expected 6 references for balance property, got: {:?}",
        locs
    );

    // Test rename on balance property
    let edit = build_rename(&doc, &workspace, None, 2, 5, "total_balance".to_string());
    assert!(edit.is_some(), "Rename should produce WorkspaceEdit");
    let ws_edit = edit.unwrap();
    let file_edits = ws_edit.changes.unwrap();
    let edits = file_edits.values().next().unwrap();
    assert_eq!(edits.len(), 6, "Rename should edit all 6 occurrences");
    for e in edits {
        assert_eq!(e.new_text, "total_balance");
    }
}

#[test]
fn test_workspace_indexing_memory_lifecycle() {
    let source = "function compute_something(): int { return 42; }";
    let uri = "file:///test/compute.vn".to_string();
    let workspace = Workspace::new();

    // 1. Indexing a workspace file must NOT keep DocumentState in workspace.files:
    workspace.index_file(uri.clone(), source.to_string());
    assert_eq!(
        workspace.file_count(),
        0,
        "index_file must not retain DocumentState in workspace.files"
    );
    assert!(
        workspace.get(&uri).is_none(),
        "workspace.get must return None for un-opened indexed files"
    );

    // But definitions must be in the project index:
    {
        let idx = workspace.index.read().unwrap();
        let defs = idx.definitions_of("compute_something");
        assert_eq!(defs.len(), 1, "Exported function must be in ProjectIndex");
        assert_eq!(defs[0].name, "compute_something");
    }

    // 2. Opening the file (update_file) puts it into workspace.files:
    workspace.update_file(uri.clone(), source.to_string());
    assert_eq!(
        workspace.file_count(),
        1,
        "update_file must keep open DocumentState in workspace.files"
    );
    assert!(workspace.get(&uri).is_some());

    // 3. Closing the file (close_file) evicts it from workspace.files, but keeps ProjectIndex:
    workspace.close_file(&uri);
    assert_eq!(
        workspace.file_count(),
        0,
        "close_file must evict DocumentState from workspace.files"
    );
    {
        let idx = workspace.index.read().unwrap();
        let defs = idx.definitions_of("compute_something");
        assert_eq!(
            defs.len(),
            1,
            "ProjectIndex must retain definitions after close"
        );
    }

    // 4. Deleting the file (remove_file) removes from both:
    workspace.remove_file(&uri);
    {
        let idx = workspace.index.read().unwrap();
        let defs = idx.definitions_of("compute_something");
        assert_eq!(defs.len(), 0, "remove_file must evict from ProjectIndex");
    }
}
