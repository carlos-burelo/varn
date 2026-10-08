use crate::document::SymbolView;
use tower_lsp_f::lsp_types::{InlayHint, InlayHintKind, Label, Position};
use varn_checker::SymbolKind;
use varn_core::ast::ExprKind;
use varn_core::TypeKind;

use crate::document::DocumentState;

pub fn build_type_hints(state: &DocumentState) -> Vec<InlayHint> {
    let mut hints = Vec::new();

    for s in state.symbols() {
        if s.line() == u32::MAX || s.is_from_stdlib() {
            continue;
        }

        match s.kind() {
            SymbolKind::Const | SymbolKind::Let | SymbolKind::Var
                if !s.has_explicit_type() && !s.type_str().is_empty() =>
            {
                let hint_col = s.col() + s.name().len() as u32;
                hints.push(InlayHint {
                    position: Position {
                        line: s.line(),
                        character: hint_col,
                    },
                    label: Label::String(format!(": {}", s.type_str())),
                    kind: Some(InlayHintKind::Type),
                    text_edits: None,
                    tooltip: None,
                    padding_left: Some(false),
                    padding_right: Some(true),
                    data: None,
                });
            }

            SymbolKind::Function | SymbolKind::Method => {
                if let Some(hint) = fn_return_hint(state, s) {
                    hints.push(hint);
                }
            }

            SymbolKind::Const
            | SymbolKind::Let
            | SymbolKind::Var
            | SymbolKind::Class
            | SymbolKind::Interface
            | SymbolKind::TypeAlias
            | SymbolKind::Enum
            | SymbolKind::Parameter
            | SymbolKind::Property
            | SymbolKind::TypeParameter
            | SymbolKind::Namespace
            | SymbolKind::Struct
            | SymbolKind::Extension
            | SymbolKind::EnumMember => {}
        }
    }

    collect_pipeline_hints(state, &mut hints);

    hints
}

fn fn_return_hint(state: &DocumentState, sym: SymbolView<'_>) -> Option<InlayHint> {
    if sym.has_explicit_type() {
        return None;
    }

    let ret_ty = varn_checker::Type::resolved(state.db.fn_shape(sym.ty())?.return_type);
    if !worth_hinting(state, &ret_ty) {
        return None;
    }
    let ret_str = state.ty_text(&ret_ty);

    let rparen_col = find_rparen_col_on_line(state, sym.line(), sym.col())?;

    Some(InlayHint {
        position: Position {
            line: sym.line(),
            character: rparen_col + 1,
        },
        label: Label::String(format!(": {ret_str}")),
        kind: Some(InlayHintKind::Type),
        text_edits: None,
        tooltip: None,
        padding_left: Some(false),
        padding_right: Some(true),
        data: None,
    })
}

fn find_rparen_col_on_line(state: &DocumentState, line: u32, after_col: u32) -> Option<u32> {
    let mut depth = 0i32;
    let mut last_rparen_col = None;
    for tok in state
        .tokens
        .iter()
        .filter(|t| t.line == line && t.col >= after_col)
    {
        match tok.kind {
            varn_core::TokenKind::LParen => depth += 1,
            varn_core::TokenKind::RParen => {
                depth -= 1;
                if depth == 0 {
                    last_rparen_col = Some(tok.col + tok.length.saturating_sub(1));
                    break;
                }
            }
            varn_core::TokenKind::EOF | varn_core::TokenKind::Dynamic | varn_core::TokenKind::Identifier
            | varn_core::TokenKind::IntegerLiteral | varn_core::TokenKind::FloatLiteral
            | varn_core::TokenKind::BinaryLiteral | varn_core::TokenKind::OctalLiteral
            | varn_core::TokenKind::HexLiteral | varn_core::TokenKind::BigIntLiteral
            | varn_core::TokenKind::Str | varn_core::TokenKind::Char
            | varn_core::TokenKind::Template | varn_core::TokenKind::TemplateHead
            | varn_core::TokenKind::TemplateMiddle | varn_core::TokenKind::TemplateTail
            | varn_core::TokenKind::RegularExpression | varn_core::TokenKind::LBrace
            | varn_core::TokenKind::RBrace | varn_core::TokenKind::LBracket
            | varn_core::TokenKind::RBracket | varn_core::TokenKind::LAngle
            | varn_core::TokenKind::RAngle | varn_core::TokenKind::Semicolon
            | varn_core::TokenKind::Comma | varn_core::TokenKind::Dot | varn_core::TokenKind::DotDot
            | varn_core::TokenKind::DotDotDot | varn_core::TokenKind::DotDotEq
            | varn_core::TokenKind::Colon | varn_core::TokenKind::ColonColon
            | varn_core::TokenKind::Question | varn_core::TokenKind::QuestionDot
            | varn_core::TokenKind::QuestionLBracket | varn_core::TokenKind::QuestionQuestion
            | varn_core::TokenKind::QuestionQuestionEq | varn_core::TokenKind::Plus
            | varn_core::TokenKind::PlusPlus | varn_core::TokenKind::PlusEq
            | varn_core::TokenKind::Minus | varn_core::TokenKind::MinusMinus
            | varn_core::TokenKind::MinusEq | varn_core::TokenKind::Star
            | varn_core::TokenKind::StarStar | varn_core::TokenKind::StarEq
            | varn_core::TokenKind::StarStarEq | varn_core::TokenKind::Slash
            | varn_core::TokenKind::SlashEq | varn_core::TokenKind::Percent
            | varn_core::TokenKind::PercentEq | varn_core::TokenKind::Amp
            | varn_core::TokenKind::AmpAmp | varn_core::TokenKind::AmpEq
            | varn_core::TokenKind::AmpAmpEq | varn_core::TokenKind::Pipe
            | varn_core::TokenKind::PipePipe | varn_core::TokenKind::PipeEq
            | varn_core::TokenKind::PipePipeEq | varn_core::TokenKind::PipeGt
            | varn_core::TokenKind::Caret | varn_core::TokenKind::CaretEq
            | varn_core::TokenKind::Tilde | varn_core::TokenKind::LtLt
            | varn_core::TokenKind::LtLtEq | varn_core::TokenKind::GtGt
            | varn_core::TokenKind::GtGtEq | varn_core::TokenKind::GtGtGt
            | varn_core::TokenKind::GtGtGtEq | varn_core::TokenKind::Eq | varn_core::TokenKind::EqEq
            | varn_core::TokenKind::EqEqEq | varn_core::TokenKind::Bang
            | varn_core::TokenKind::BangEq | varn_core::TokenKind::BangEqEq
            | varn_core::TokenKind::Lt | varn_core::TokenKind::LtEq | varn_core::TokenKind::Gt
            | varn_core::TokenKind::GtEq | varn_core::TokenKind::Arrow
            | varn_core::TokenKind::FatArrow | varn_core::TokenKind::Let
            | varn_core::TokenKind::Const | varn_core::TokenKind::Var
            | varn_core::TokenKind::Function | varn_core::TokenKind::Class
            | varn_core::TokenKind::Struct | varn_core::TokenKind::Interface
            | varn_core::TokenKind::Type | varn_core::TokenKind::Enum
            | varn_core::TokenKind::Namespace | varn_core::TokenKind::Module
            | varn_core::TokenKind::Extension | varn_core::TokenKind::On | varn_core::TokenKind::If
            | varn_core::TokenKind::Else | varn_core::TokenKind::Switch
            | varn_core::TokenKind::Case | varn_core::TokenKind::Default
            | varn_core::TokenKind::While | varn_core::TokenKind::For | varn_core::TokenKind::Do
            | varn_core::TokenKind::Break | varn_core::TokenKind::Continue
            | varn_core::TokenKind::Return | varn_core::TokenKind::Throw | varn_core::TokenKind::Try
            | varn_core::TokenKind::Catch | varn_core::TokenKind::Finally
            | varn_core::TokenKind::Using | varn_core::TokenKind::With
            | varn_core::TokenKind::Import | varn_core::TokenKind::Export
            | varn_core::TokenKind::From | varn_core::TokenKind::As | varn_core::TokenKind::Async
            | varn_core::TokenKind::Await | varn_core::TokenKind::Yield | varn_core::TokenKind::New
            | varn_core::TokenKind::This | varn_core::TokenKind::Super
            | varn_core::TokenKind::Delete | varn_core::TokenKind::Typeof
            | varn_core::TokenKind::Instanceof | varn_core::TokenKind::In | varn_core::TokenKind::Of
            | varn_core::TokenKind::Void | varn_core::TokenKind::Is | varn_core::TokenKind::True
            | varn_core::TokenKind::False | varn_core::TokenKind::Null
            | varn_core::TokenKind::Public | varn_core::TokenKind::Private
            | varn_core::TokenKind::Protected | varn_core::TokenKind::Static
            | varn_core::TokenKind::Abstract | varn_core::TokenKind::Override
            | varn_core::TokenKind::Readonly | varn_core::TokenKind::Declare
            | varn_core::TokenKind::Native | varn_core::TokenKind::Extends
            | varn_core::TokenKind::Implements | varn_core::TokenKind::Get
            | varn_core::TokenKind::Set | varn_core::TokenKind::Constructor
            | varn_core::TokenKind::Destructor | varn_core::TokenKind::Match
            | varn_core::TokenKind::At | varn_core::TokenKind::Hash
            | varn_core::TokenKind::Backslash | varn_core::TokenKind::Dollar
            | varn_core::TokenKind::Backtick | varn_core::TokenKind::Newline
            | varn_core::TokenKind::Whitespace | varn_core::TokenKind::DocComment
            | varn_core::TokenKind::Placeholder | varn_core::TokenKind::DecimalLiteral
            | varn_core::TokenKind::Spawn | varn_core::TokenKind::Parallel
            | varn_core::TokenKind::Start | varn_core::TokenKind::RawStr => {}
        }
    }
    last_rparen_col
}

fn worth_hinting(state: &DocumentState, ty: &varn_checker::Type) -> bool {
    !matches!(
        state.db.ty_kind(ty),
        TypeKind::Primitive(varn_core::LangPrimitive::Void | varn_core::LangPrimitive::Dynamic)
    )
}

fn collect_pipeline_hints(state: &DocumentState, hints: &mut Vec<InlayHint>) {
    let arena = &state.ast_arena;
    for expr in state.spatial_index.exprs() {
        let ExprKind::Pipeline { right, .. } = &arena.expr(expr).kind else {
            continue;
        };
        let Some(entry) = state.db.expr_table.get(&expr.index()) else {
            continue;
        };
        if !worth_hinting(state, &entry.ty) {
            continue;
        }
        let r_end = &arena.expr(*right).range.end;
        hints.push(InlayHint {
            position: Position {
                line: r_end.line.saturating_sub(1),
                character: r_end.column,
            },
            label: Label::String(format!(": {}", state.ty_text(&entry.ty))),
            kind: Some(InlayHintKind::Type),
            text_edits: None,
            tooltip: None,
            padding_left: Some(true),
            padding_right: Some(false),
            data: None,
        });
    }
}
