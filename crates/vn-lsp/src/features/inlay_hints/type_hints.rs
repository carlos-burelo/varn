use crate::document::SymbolView;
use tower_lsp_f::lsp_types::{InlayHint, InlayHintKind, Label, Position};
use vn_core::ast::ExprKind;
use vn_core::TypeKind;
use vn_sem::symbol::SymbolKind;

use crate::document::DocumentState;

pub fn build_type_hints(state: &DocumentState) -> Vec<InlayHint> {
    let mut hints = Vec::new();

    for s in state.symbols() {
        if s.line() == u32::MAX
            || s.is_from_stdlib()
            || vn_core::intrinsics::is_synthetic_name(s.name())
        {
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

    let ret_ty = vn_sem::types::Type::resolved(state.db.fn_shape(sym.ty())?.return_type);
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
            vn_core::TokenKind::LParen => depth += 1,
            vn_core::TokenKind::RParen => {
                depth -= 1;
                if depth == 0 {
                    last_rparen_col = Some(tok.col + tok.length.saturating_sub(1));
                    break;
                }
            }
            vn_core::TokenKind::EOF
            | vn_core::TokenKind::Dynamic
            | vn_core::TokenKind::Identifier
            | vn_core::TokenKind::IntegerLiteral
            | vn_core::TokenKind::FloatLiteral
            | vn_core::TokenKind::BinaryLiteral
            | vn_core::TokenKind::OctalLiteral
            | vn_core::TokenKind::HexLiteral
            | vn_core::TokenKind::BigIntLiteral
            | vn_core::TokenKind::Str
            | vn_core::TokenKind::Char
            | vn_core::TokenKind::Template
            | vn_core::TokenKind::TemplateHead
            | vn_core::TokenKind::TemplateMiddle
            | vn_core::TokenKind::TemplateTail
            | vn_core::TokenKind::RegularExpression
            | vn_core::TokenKind::LBrace
            | vn_core::TokenKind::RBrace
            | vn_core::TokenKind::LBracket
            | vn_core::TokenKind::RBracket
            | vn_core::TokenKind::LAngle
            | vn_core::TokenKind::RAngle
            | vn_core::TokenKind::Semicolon
            | vn_core::TokenKind::Comma
            | vn_core::TokenKind::Dot
            | vn_core::TokenKind::DotDot
            | vn_core::TokenKind::DotDotDot
            | vn_core::TokenKind::DotDotEq
            | vn_core::TokenKind::Colon
            | vn_core::TokenKind::ColonColon
            | vn_core::TokenKind::Question
            | vn_core::TokenKind::QuestionDot
            | vn_core::TokenKind::QuestionLBracket
            | vn_core::TokenKind::QuestionQuestion
            | vn_core::TokenKind::QuestionQuestionEq
            | vn_core::TokenKind::Plus
            | vn_core::TokenKind::PlusPlus
            | vn_core::TokenKind::PlusEq
            | vn_core::TokenKind::Minus
            | vn_core::TokenKind::MinusMinus
            | vn_core::TokenKind::MinusEq
            | vn_core::TokenKind::Star
            | vn_core::TokenKind::StarStar
            | vn_core::TokenKind::StarEq
            | vn_core::TokenKind::StarStarEq
            | vn_core::TokenKind::Slash
            | vn_core::TokenKind::SlashEq
            | vn_core::TokenKind::Percent
            | vn_core::TokenKind::PercentEq
            | vn_core::TokenKind::Amp
            | vn_core::TokenKind::AmpAmp
            | vn_core::TokenKind::AmpEq
            | vn_core::TokenKind::AmpAmpEq
            | vn_core::TokenKind::Pipe
            | vn_core::TokenKind::PipePipe
            | vn_core::TokenKind::PipeEq
            | vn_core::TokenKind::PipePipeEq
            | vn_core::TokenKind::PipeGt
            | vn_core::TokenKind::Caret
            | vn_core::TokenKind::CaretEq
            | vn_core::TokenKind::Tilde
            | vn_core::TokenKind::LtLt
            | vn_core::TokenKind::LtLtEq
            | vn_core::TokenKind::GtGt
            | vn_core::TokenKind::GtGtEq
            | vn_core::TokenKind::GtGtGt
            | vn_core::TokenKind::GtGtGtEq
            | vn_core::TokenKind::Eq
            | vn_core::TokenKind::EqEq
            | vn_core::TokenKind::EqEqEq
            | vn_core::TokenKind::Bang
            | vn_core::TokenKind::BangEq
            | vn_core::TokenKind::BangEqEq
            | vn_core::TokenKind::Lt
            | vn_core::TokenKind::LtEq
            | vn_core::TokenKind::Gt
            | vn_core::TokenKind::GtEq
            | vn_core::TokenKind::Arrow
            | vn_core::TokenKind::FatArrow
            | vn_core::TokenKind::Let
            | vn_core::TokenKind::Const
            | vn_core::TokenKind::Var
            | vn_core::TokenKind::Function
            | vn_core::TokenKind::Class
            | vn_core::TokenKind::Struct
            | vn_core::TokenKind::Interface
            | vn_core::TokenKind::Type
            | vn_core::TokenKind::Enum
            | vn_core::TokenKind::Namespace
            | vn_core::TokenKind::Module
            | vn_core::TokenKind::Extension
            | vn_core::TokenKind::On
            | vn_core::TokenKind::If
            | vn_core::TokenKind::Else
            | vn_core::TokenKind::Switch
            | vn_core::TokenKind::Case
            | vn_core::TokenKind::Default
            | vn_core::TokenKind::While
            | vn_core::TokenKind::For
            | vn_core::TokenKind::Do
            | vn_core::TokenKind::Break
            | vn_core::TokenKind::Continue
            | vn_core::TokenKind::Return
            | vn_core::TokenKind::Throw
            | vn_core::TokenKind::Try
            | vn_core::TokenKind::Catch
            | vn_core::TokenKind::Finally
            | vn_core::TokenKind::Using
            | vn_core::TokenKind::With
            | vn_core::TokenKind::Import
            | vn_core::TokenKind::Export
            | vn_core::TokenKind::From
            | vn_core::TokenKind::As
            | vn_core::TokenKind::Async
            | vn_core::TokenKind::Await
            | vn_core::TokenKind::Yield
            | vn_core::TokenKind::New
            | vn_core::TokenKind::This
            | vn_core::TokenKind::Super
            | vn_core::TokenKind::Delete
            | vn_core::TokenKind::Typeof
            | vn_core::TokenKind::Instanceof
            | vn_core::TokenKind::In
            | vn_core::TokenKind::Of
            | vn_core::TokenKind::Void
            | vn_core::TokenKind::Is
            | vn_core::TokenKind::True
            | vn_core::TokenKind::False
            | vn_core::TokenKind::Null
            | vn_core::TokenKind::Public
            | vn_core::TokenKind::Private
            | vn_core::TokenKind::Protected
            | vn_core::TokenKind::Static
            | vn_core::TokenKind::Abstract
            | vn_core::TokenKind::Override
            | vn_core::TokenKind::Readonly
            | vn_core::TokenKind::Mut
            | vn_core::TokenKind::Out
            | vn_core::TokenKind::Declare
            | vn_core::TokenKind::Native
            | vn_core::TokenKind::Extends
            | vn_core::TokenKind::Implements
            | vn_core::TokenKind::Get
            | vn_core::TokenKind::Set
            | vn_core::TokenKind::Constructor
            | vn_core::TokenKind::Destructor
            | vn_core::TokenKind::Match
            | vn_core::TokenKind::At
            | vn_core::TokenKind::Hash
            | vn_core::TokenKind::Backslash
            | vn_core::TokenKind::Dollar
            | vn_core::TokenKind::Backtick
            | vn_core::TokenKind::Newline
            | vn_core::TokenKind::Whitespace
            | vn_core::TokenKind::DocComment
            | vn_core::TokenKind::Placeholder
            | vn_core::TokenKind::DecimalLiteral
            | vn_core::TokenKind::Spawn
            | vn_core::TokenKind::Parallel
            | vn_core::TokenKind::Start
            | vn_core::TokenKind::RawStr => {}
        }
    }
    last_rparen_col
}

fn worth_hinting(state: &DocumentState, ty: &vn_sem::types::Type) -> bool {
    !matches!(
        state.db.ty_kind(ty),
        TypeKind::Primitive(
            vn_core::LangPrimitive::Void
                | vn_core::LangPrimitive::Dynamic
                | vn_core::LangPrimitive::Error
        )
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
