#![allow(unused_crate_dependencies)]

//! Half-typed source is the dominant input of a language server, and until the
//! parser grew `ExprKind::Missing` / `StmtKind::Error` it was the one input
//! nothing covered.
//!
//! What these lock down is a single property, stated three ways: **an
//! incomplete construct must not damage what surrounds it.** The symbols before
//! it still bind, the declarations after it still parse, and the checker stays
//! quiet about the hole itself so the editor is not painted red mid-keystroke.

use varn_checker::SymbolKind;
use varn_lsp::features::completion::members::dot_receiver;
use varn_lsp::pipeline::run_pipeline;

fn analyze(source: &str) -> varn_lsp::document::DocumentAnalysis {
    run_pipeline(source.to_string(), "file:///test/incomplete.vn".to_string())
}

/// The regression that motivated the whole change: `g.` followed by a newline
/// used to consume the *next line's* `const` as the property name. That single
/// mis-parse produced `Member(g, "const")`, destroyed the declaration of `m`,
/// and reported `property 'const' does not exist on type 'str'` — an error
/// naming a keyword the user never typed as a property.
#[test]
fn dangling_dot_does_not_swallow_the_next_declaration() {
    let state = analyze("const g: str = \"hola\"\nconst n = g.\nconst m = 42\n");

    let m = state
        .symbols
        .iter()
        .find(|s| s.name == "m")
        .expect("`m` must still be declared: the dangling dot on the line above must not eat it");
    assert_eq!(
        m.kind,
        SymbolKind::Const,
        "`m` must bind as a const declaration, not as a bare assignment expression"
    );

    assert!(
        !state
            .diagnostics
            .iter()
            .any(|d| d.message.contains("'const'")),
        "no diagnostic may name the `const` keyword as a property; got: {:?}",
        state
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// `g.` at end of input. The hole itself is reported by the parser exactly
/// once; the checker must not pile a second, misleading error on top of it.
#[test]
fn dangling_dot_at_eof_reports_the_hole_and_nothing_else() {
    let state = analyze("const g: str = \"hola\"\nconst n = g.\n");

    assert!(
        state
            .diagnostics
            .iter()
            .any(|d| d.message.contains("expected a property name")),
        "the missing property must be reported; got: {:?}",
        state
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    assert!(
        !state
            .diagnostics
            .iter()
            .any(|d| d.message.contains("does not exist on type")),
        "the checker must stay silent about a hole the parser already reported; got: {:?}",
        state
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// The payoff. Completion at `g.<cursor>` has to know the receiver is `str`,
/// and it must learn that from the checker's type for `g` — which only exists
/// because the enclosing declaration survived the hole.
#[test]
fn dangling_dot_still_yields_a_typed_receiver_for_completion() {
    let source = "const g: str = \"hola\"\nconst n = g.\n";
    let state = analyze(source);

    let receiver = dot_receiver(&state, 1, 12, Some("."))
        .expect("a receiver must be resolvable at `g.` for member completion");

    let ty = match receiver {
        varn_lsp::features::completion::members::ReceiverInfo::Typed { ty, .. } => ty.to_string(),
        varn_lsp::features::completion::members::ReceiverInfo::Named { name, .. } => name,
        varn_lsp::features::completion::members::ReceiverInfo::Anonymous(_) => {
            panic!("receiver of `g.` must be the named type `str`, not an anonymous object")
        }
    };
    assert_eq!(ty, "str", "the receiver of `g.` must be typed as `str`");
}

/// A statement the parser cannot make sense of is preserved as
/// `StmtKind::Error` rather than dropped, so recovery cannot silently take the
/// declarations around it with it.
#[test]
fn unparseable_statement_does_not_drop_its_neighbours() {
    let state = analyze("const a = 1\nconst b = \nconst c = 3\n");

    for name in ["a", "c"] {
        assert!(
            state.symbols.iter().any(|s| s.name == name),
            "`{name}` must survive recovery from the malformed declaration between them; \
             bound symbols: {:?}",
            state.symbols.iter().map(|s| &s.name).collect::<Vec<_>>()
        );
    }
}

/// Recovery must always consume ground. A statement keyword that fails to parse
/// is also a recovery boundary, so without a forced advance the program loop
/// would retry the same token forever.
#[test]
fn recovery_terminates_on_a_statement_keyword_that_fails_to_parse() {
    // No assertion beyond returning: a hang here is the failure.
    let state = analyze("const\nconst\nconst\n");
    let _ = state.symbols.len();
}
