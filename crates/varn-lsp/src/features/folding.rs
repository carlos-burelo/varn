use tower_lsp::lsp_types::{FoldingRange, FoldingRangeKind};
use varn_core::{TokenKind, Trivia, TriviaKind};

use crate::document::{DocumentState, TokenRecord};

pub fn build_folding_ranges(state: &DocumentState) -> Vec<FoldingRange> {
    let mut ranges = fold_tokens(&state.tokens);
    ranges.extend(fold_comments(&state.trivia));
    ranges
}

/// Foldable comment regions: one per multi-line `/* */`, and one per run of
/// consecutive `//` lines.
///
/// Only possible now that the scanner records comments — before, they were
/// dropped during lexing, so `FoldingRangeKind::Comment` had nothing to fold
/// and went unused.
pub fn fold_comments(trivia: &[Trivia]) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();
    let mut run: Option<(u32, u32)> = None;

    for t in trivia {
        // Trivia ranges are 1-based (lexer convention); LSP lines are 0-based.
        let start = t.range.start.line.saturating_sub(1);
        let end = t.range.end.line.saturating_sub(1);

        match t.kind {
            TriviaKind::Block => {
                if let Some((s, e)) = run.take() {
                    push_comment_fold(&mut ranges, s, e);
                }
                if end > start {
                    push_comment_fold(&mut ranges, start, end);
                }
            }
            TriviaKind::Line => match run {
                // Adjacent line comments fold as one block; a gap ends the run.
                Some((s, e)) if start == e + 1 => run = Some((s, start)),
                Some((s, e)) => {
                    push_comment_fold(&mut ranges, s, e);
                    run = Some((start, start));
                }
                None => run = Some((start, start)),
            },
        }
    }

    if let Some((s, e)) = run {
        push_comment_fold(&mut ranges, s, e);
    }
    ranges
}

fn push_comment_fold(ranges: &mut Vec<FoldingRange>, start_line: u32, end_line: u32) {
    if end_line > start_line {
        ranges.push(fold(start_line, end_line, Some(FoldingRangeKind::Comment)));
    }
}

pub fn fold_tokens(tokens: &[TokenRecord]) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();
    let mut brace_stack: Vec<(u32, usize)> = Vec::new();
    let mut bracket_stack: Vec<u32> = Vec::new();

    let import_lines = collect_import_line_ranges(tokens);

    for (i, tok) in tokens.iter().enumerate() {
        match tok.kind {
            TokenKind::LBrace => brace_stack.push((tok.line, i)),
            TokenKind::RBrace => {
                if let Some((start, open_idx)) = brace_stack.pop() {
                    if tok.line > start {
                        let kind = classify_brace_kind(tokens, open_idx, start, &import_lines);
                        ranges.push(fold(start, tok.line, kind));
                    }
                }
            }
            TokenKind::LBracket => bracket_stack.push(tok.line),
            TokenKind::RBracket => {
                if let Some(start) = bracket_stack.pop() {
                    if tok.line > start {
                        ranges.push(fold(start, tok.line, None));
                    }
                }
            }
            _ => {}
        }
    }

    ranges
}

fn classify_brace_kind(
    tokens: &[TokenRecord],
    open_idx: usize,
    brace_line: u32,
    import_lines: &[(u32, u32)],
) -> Option<FoldingRangeKind> {
    for &(start, end) in import_lines {
        if brace_line >= start && brace_line <= end {
            return Some(FoldingRangeKind::Imports);
        }
    }

    let trigger = tokens[..open_idx]
        .iter()
        .rev()
        .take_while(|t| t.line == brace_line)
        .find(|t| {
            matches!(
                t.kind,
                TokenKind::Function
                    | TokenKind::FatArrow
                    | TokenKind::Class
                    | TokenKind::Interface
                    | TokenKind::Namespace
                    | TokenKind::Enum
            ) || (t.kind == TokenKind::Identifier && is_region_keyword(&t.lexeme))
        });

    if trigger.is_some() {
        Some(FoldingRangeKind::Region)
    } else {
        None
    }
}

fn is_region_keyword(lexeme: &str) -> bool {
    matches!(
        lexeme,
        "function" | "class" | "interface" | "namespace" | "enum"
    )
}

fn collect_import_line_ranges(tokens: &[TokenRecord]) -> Vec<(u32, u32)> {
    let mut import_lines: Vec<u32> = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Import)
        .map(|t| t.line)
        .collect();
    import_lines.dedup();

    if import_lines.is_empty() {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    let mut start = import_lines[0];
    let mut prev = import_lines[0];
    for &ln in &import_lines[1..] {
        if ln > prev + 5 {
            if prev > start {
                ranges.push((start, prev));
            }
            start = ln;
        }
        prev = ln;
    }
    if prev > start {
        ranges.push((start, prev));
    }
    ranges
}

#[inline]
fn fold(start_line: u32, end_line: u32, kind: Option<FoldingRangeKind>) -> FoldingRange {
    FoldingRange {
        start_line,
        start_character: None,
        end_line,
        end_character: None,
        kind,
        collapsed_text: None,
    }
}
