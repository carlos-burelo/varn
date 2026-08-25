#![allow(unused_crate_dependencies)]

//! The document outline lists what the file declares — not what the types of
//! those declarations contain.
//!
//! Routing the outline's children through the checker's member API made that
//! distinction load-bearing: asking `get_members_of_type` about a `const s: str`
//! answers with every method on `str`, so the outline briefly hung forty-odd
//! stdlib entries under a one-line declaration. Only a container nests.

use tower_lsp::lsp_types::{DocumentSymbol, DocumentSymbolResponse};
use varn_lsp::features::symbols::build_document_symbols;
use varn_lsp::pipeline::run_pipeline;

const SRC: &str = "class Widget {\n\
                   \x20   value: int = 0\n\
                   \x20   bump(n: int): int { return this.value + n }\n\
                   }\n\
                   const w = new Widget()\n\
                   const s: str = \"hola\"\n";

fn outline() -> Vec<DocumentSymbol> {
    let state = run_pipeline(SRC.to_owned(), "file:///test/outline.vn".to_owned());
    match build_document_symbols(&state) {
        DocumentSymbolResponse::Nested(v) => v,
        DocumentSymbolResponse::Flat(_) => panic!("the server replies with a nested outline"),
    }
}

fn find<'a>(nodes: &'a [DocumentSymbol], name: &str) -> Option<&'a DocumentSymbol> {
    nodes.iter().find(|n| n.name == name)
}

/// The regression. `s` is one entry, not a folder of `str`'s methods.
#[test]
fn a_variable_does_not_nest_the_members_of_its_type() {
    let nodes = outline();

    for name in ["w", "s"] {
        let node = find(&nodes, name).unwrap_or_else(|| panic!("`{name}` must be in the outline"));
        let children = node.children.as_ref().map(Vec::len).unwrap_or(0);
        assert_eq!(
            children, 0,
            "`{name}` is a variable: its type's members belong to the type's own entry, \
             not hanging under the variable"
        );
    }
}

/// The other half: a container still nests what it declares, so a fix for the
/// above cannot be "stop nesting entirely".
#[test]
fn a_class_nests_the_members_it_declares() {
    let nodes = outline();
    let widget = find(&nodes, "Widget").expect("`Widget` must be in the outline");

    let names: Vec<&str> = widget
        .children
        .as_ref()
        .map(|c| c.iter().map(|n| n.name.as_str()).collect())
        .unwrap_or_default();

    assert!(
        names.contains(&"value") && names.contains(&"bump"),
        "a class must nest its own members; got {names:?}"
    );
}
