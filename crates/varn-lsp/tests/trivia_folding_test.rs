#![allow(unused_crate_dependencies)]

//! Comments used to be dropped during lexing, which made them unrecoverable
//! downstream — `FoldingRangeKind::Comment` existed and nothing could ever
//! produce it. These lock down the two halves of the fix: the scanner records
//! comments on a stream parallel to the tokens, and folding consumes them.
//!
//! The parallel stream is the load-bearing part. A comment can appear anywhere
//! (`a + /* c */ b`), so interleaving trivia into the token vector would force
//! the parser to filter on every `peek`. `token_stream_stays_free_of_trivia`
//! is what keeps that from silently regressing.

use tower_lsp::lsp_types::FoldingRangeKind;
use varn_core::TriviaKind;
use varn_lsp::features::folding::build_folding_ranges;
use varn_lsp::pipeline::run_pipeline;

fn analyze(source: &str) -> varn_lsp::document::DocumentAnalysis {
    run_pipeline(source.to_string(), "file:///test/trivia.vn".to_string())
}

#[test]
fn line_and_block_comments_are_recorded_with_their_kinds() {
    let state = analyze("// leading\nconst a = 1 /* trailing */\n");

    let kinds: Vec<TriviaKind> = state.trivia.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![TriviaKind::Line, TriviaKind::Block],
        "both comment forms must be recorded, in source order"
    );
}

/// Doc comments are deliberately *not* trivia: the parser attaches them to the
/// declaration that follows, so they travel in the main token stream.
#[test]
fn doc_comments_are_not_trivia() {
    let state = analyze("/** documented */\nfunction f(): void {}\n");

    assert!(
        state.trivia.is_empty(),
        "a doc comment belongs to the token stream, not the trivia stream; got {:?}",
        state.trivia.iter().map(|t| t.kind).collect::<Vec<_>>()
    );
}

/// The invariant that lets the parser stay untouched: trivia never enters the
/// token vector, not even for a comment sitting mid-expression.
#[test]
fn token_stream_stays_free_of_trivia() {
    let state = analyze("const a = 1 /* inline */ + 2\n");

    assert!(
        !state.tokens.iter().any(|t| t.lexeme.contains("inline")),
        "the comment must not appear in the token stream; tokens: {:?}",
        state.tokens.iter().map(|t| &t.lexeme).collect::<Vec<_>>()
    );
    assert_eq!(
        state.trivia.len(),
        1,
        "the inline comment must still be recorded as trivia"
    );
    assert!(
        state.diagnostics.is_empty(),
        "a mid-expression comment must not disturb parsing; got: {:?}",
        state
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_run_of_line_comments_folds_as_one_region() {
    // Lines 0-2 are one run; line 4 is separated by a blank line and, being a
    // single line, is not foldable at all.
    let state = analyze("// one\n// two\n// three\n\n// lonely\nconst a = 1\n");

    let comment_folds: Vec<(u32, u32)> = build_folding_ranges(&state)
        .into_iter()
        .filter(|r| r.kind == Some(FoldingRangeKind::Comment))
        .map(|r| (r.start_line, r.end_line))
        .collect();

    assert_eq!(
        comment_folds,
        vec![(0, 2)],
        "consecutive line comments fold together; a blank line ends the run and \
         a single line is not foldable"
    );
}

#[test]
fn a_multiline_block_comment_folds() {
    let state = analyze("/*\n  spanning\n  lines\n*/\nconst a = 1\n");

    let comment_folds: Vec<(u32, u32)> = build_folding_ranges(&state)
        .into_iter()
        .filter(|r| r.kind == Some(FoldingRangeKind::Comment))
        .map(|r| (r.start_line, r.end_line))
        .collect();

    assert_eq!(comment_folds, vec![(0, 3)]);
}

/// Folding of braces and brackets predates comment folding and must survive it.
#[test]
fn comment_folding_does_not_displace_brace_folding() {
    let state = analyze("// header\n// header\nfunction f(): void {\n  const a = 1\n}\n");

    let ranges = build_folding_ranges(&state);
    assert!(
        ranges
            .iter()
            .any(|r| r.kind == Some(FoldingRangeKind::Comment)),
        "the comment run must fold"
    );
    assert!(
        ranges
            .iter()
            .any(|r| r.kind != Some(FoldingRangeKind::Comment)),
        "the function body must still fold; got: {:?}",
        ranges
            .iter()
            .map(|r| (r.start_line, r.end_line, r.kind.clone()))
            .collect::<Vec<_>>()
    );
}
