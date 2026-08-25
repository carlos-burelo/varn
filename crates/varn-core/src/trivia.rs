use crate::source::SourceRange;

/// Source the parser discards but tooling cannot reconstruct.
///
/// **Only comments are recorded.** Whitespace is deliberately absent: it is
/// exactly the gap between adjacent token ranges, so it stays derivable from
/// the token stream plus the source text. Comments are not derivable — the
/// scanner skips them outright — which makes them the one thing that has to be
/// captured for every byte of source to remain accounted for.
///
/// Trivia is delivered on a stream **parallel** to the tokens, never mixed into
/// them. A comment can appear anywhere (`a + /* c */ b`), so interleaving would
/// force every `peek`/`advance` in the parser's hot path to filter. The parser
/// tolerates `TokenKind::DocComment` inline only because doc comments are legal
/// in just three positions, where it checks for them explicitly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriviaKind {
    /// `// …` up to (not including) the end of the line.
    Line,
    /// `/* … */`, possibly spanning lines.
    ///
    /// Doc comments (`/** … */`) are **not** trivia: they reach the parser as
    /// `TokenKind::DocComment` in the main stream, because the parser attaches
    /// them to the declaration that follows.
    Block,
}

/// One comment, as a range into the source that produced it.
///
/// Carries no text: every consumer already holds the source, and copying the
/// bytes would duplicate them for no gain.
#[derive(Clone, Copy, Debug)]
pub struct Trivia {
    pub kind: TriviaKind,
    pub range: SourceRange,
}
