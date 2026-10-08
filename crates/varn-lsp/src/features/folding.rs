use tower_lsp_f::lsp_types::{FoldingRange, FoldingRangeKind};
use varn_core::{TokenKind, Trivia, TriviaKind};

use crate::document::{DocumentState, TokenRecord};

pub fn build_folding_ranges(state: &DocumentState) -> Vec<FoldingRange> {
    let mut ranges = fold_tokens(&state.source, &state.tokens);
    ranges.extend(fold_comments(&state.trivia));
    ranges
}

pub fn fold_comments(trivia: &[Trivia]) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();
    let mut run: Option<(u32, u32)> = None;

    for t in trivia {
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

pub fn fold_tokens(source: &str, tokens: &[TokenRecord]) -> Vec<FoldingRange> {
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
                        let kind =
                            classify_brace_kind(source, tokens, open_idx, start, &import_lines);
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
            TokenKind::EOF | TokenKind::Dynamic | TokenKind::Identifier | TokenKind::IntegerLiteral
            | TokenKind::FloatLiteral | TokenKind::BinaryLiteral | TokenKind::OctalLiteral
            | TokenKind::HexLiteral | TokenKind::BigIntLiteral | TokenKind::Str | TokenKind::Char
            | TokenKind::Template | TokenKind::TemplateHead | TokenKind::TemplateMiddle
            | TokenKind::TemplateTail | TokenKind::RegularExpression | TokenKind::LParen
            | TokenKind::RParen | TokenKind::LAngle | TokenKind::RAngle | TokenKind::Semicolon
            | TokenKind::Comma | TokenKind::Dot | TokenKind::DotDot | TokenKind::DotDotDot
            | TokenKind::DotDotEq | TokenKind::Colon | TokenKind::ColonColon | TokenKind::Question
            | TokenKind::QuestionDot | TokenKind::QuestionLBracket | TokenKind::QuestionQuestion
            | TokenKind::QuestionQuestionEq | TokenKind::Plus | TokenKind::PlusPlus
            | TokenKind::PlusEq | TokenKind::Minus | TokenKind::MinusMinus | TokenKind::MinusEq
            | TokenKind::Star | TokenKind::StarStar | TokenKind::StarEq | TokenKind::StarStarEq
            | TokenKind::Slash | TokenKind::SlashEq | TokenKind::Percent | TokenKind::PercentEq
            | TokenKind::Amp | TokenKind::AmpAmp | TokenKind::AmpEq | TokenKind::AmpAmpEq
            | TokenKind::Pipe | TokenKind::PipePipe | TokenKind::PipeEq | TokenKind::PipePipeEq
            | TokenKind::PipeGt | TokenKind::Caret | TokenKind::CaretEq | TokenKind::Tilde
            | TokenKind::LtLt | TokenKind::LtLtEq | TokenKind::GtGt | TokenKind::GtGtEq
            | TokenKind::GtGtGt | TokenKind::GtGtGtEq | TokenKind::Eq | TokenKind::EqEq
            | TokenKind::EqEqEq | TokenKind::Bang | TokenKind::BangEq | TokenKind::BangEqEq
            | TokenKind::Lt | TokenKind::LtEq | TokenKind::Gt | TokenKind::GtEq | TokenKind::Arrow
            | TokenKind::FatArrow | TokenKind::Let | TokenKind::Const | TokenKind::Var
            | TokenKind::Function | TokenKind::Class | TokenKind::Struct | TokenKind::Interface
            | TokenKind::Type | TokenKind::Enum | TokenKind::Namespace | TokenKind::Module
            | TokenKind::Extension | TokenKind::On | TokenKind::If | TokenKind::Else
            | TokenKind::Switch | TokenKind::Case | TokenKind::Default | TokenKind::While
            | TokenKind::For | TokenKind::Do | TokenKind::Break | TokenKind::Continue
            | TokenKind::Return | TokenKind::Throw | TokenKind::Try | TokenKind::Catch
            | TokenKind::Finally | TokenKind::Using | TokenKind::With | TokenKind::Import
            | TokenKind::Export | TokenKind::From | TokenKind::As | TokenKind::Async
            | TokenKind::Await | TokenKind::Yield | TokenKind::New | TokenKind::This
            | TokenKind::Super | TokenKind::Delete | TokenKind::Typeof | TokenKind::Instanceof
            | TokenKind::In | TokenKind::Of | TokenKind::Void | TokenKind::Is | TokenKind::True
            | TokenKind::False | TokenKind::Null | TokenKind::Public | TokenKind::Private
            | TokenKind::Protected | TokenKind::Static | TokenKind::Abstract | TokenKind::Override
            | TokenKind::Readonly | TokenKind::Declare | TokenKind::Native | TokenKind::Extends
            | TokenKind::Implements | TokenKind::Get | TokenKind::Set | TokenKind::Constructor
            | TokenKind::Destructor | TokenKind::Match | TokenKind::At | TokenKind::Hash
            | TokenKind::Backslash | TokenKind::Dollar | TokenKind::Backtick | TokenKind::Newline
            | TokenKind::Whitespace | TokenKind::DocComment | TokenKind::Placeholder
            | TokenKind::DecimalLiteral | TokenKind::Spawn | TokenKind::Parallel | TokenKind::Start
            | TokenKind::RawStr => {}
        }
    }

    ranges
}

fn classify_brace_kind(
    source: &str,
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
            ) || (t.kind == TokenKind::Identifier
                && is_region_keyword(crate::document::token_lexeme(source, t)))
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
