mod classify;
mod context;
mod modifiers;

use once_cell::sync::Lazy;
use tower_lsp_f::lsp_types::{SemanticTokenModifiers, SemanticTokenTypes, SemanticTokensLegend};
use vn_core::TokenKind;

use crate::document::DocumentState;

pub static LEGEND: Lazy<SemanticTokensLegend> = Lazy::new(|| SemanticTokensLegend {
    token_types: vec![
        SemanticTokenTypes::Keyword.into(),
        SemanticTokenTypes::Type.into(),
        SemanticTokenTypes::Variable.into(),
        SemanticTokenTypes::Function.into(),
        SemanticTokenTypes::Class.into(),
        SemanticTokenTypes::Parameter.into(),
        SemanticTokenTypes::Property.into(),
        SemanticTokenTypes::Number.into(),
        SemanticTokenTypes::String.into(),
        SemanticTokenTypes::EnumMember.into(),
        SemanticTokenTypes::Namespace.into(),
        SemanticTokenTypes::Interface.into(),
        SemanticTokenTypes::TypeParameter.into(),
        SemanticTokenTypes::Method.into(),
        SemanticTokenTypes::Enum.into(),
        SemanticTokenTypes::Decorator.into(),
        SemanticTokenTypes::Regexp.into(),
    ],
    token_modifiers: vec![
        SemanticTokenModifiers::Declaration.into(),
        SemanticTokenModifiers::Readonly.into(),
        SemanticTokenModifiers::Async.into(),
        SemanticTokenModifiers::Static.into(),
        SemanticTokenModifiers::Abstract.into(),
        SemanticTokenModifiers::Deprecated.into(),
        SemanticTokenModifiers::DefaultLibrary.into(),
    ],
});

pub const TT_KEYWORD: u32 = 0;
pub const TT_TYPE: u32 = 1;
pub const TT_VARIABLE: u32 = 2;
pub const TT_FUNCTION: u32 = 3;
pub const TT_CLASS: u32 = 4;
pub const TT_PARAMETER: u32 = 5;
pub const TT_PROPERTY: u32 = 6;
pub const TT_NUMBER: u32 = 7;
pub const TT_STRING: u32 = 8;
pub const TT_ENUM_MEMBER: u32 = 9;
pub const TT_NAMESPACE: u32 = 10;
pub const TT_INTERFACE: u32 = 11;
pub const TT_TYPE_PARAMETER: u32 = 12;
pub const TT_METHOD: u32 = 13;
pub const TT_ENUM: u32 = 14;
pub const TT_DECORATOR: u32 = 15;
pub const TT_REGEXP: u32 = 16;

pub const MOD_DECLARATION: u32 = 1 << 0;
pub const MOD_READONLY: u32 = 1 << 1;
pub const MOD_ASYNC: u32 = 1 << 2;
pub const MOD_STATIC: u32 = 1 << 3;
pub const MOD_ABSTRACT: u32 = 1 << 4;
pub const MOD_DEPRECATED: u32 = 1 << 5;
pub const MOD_DEFAULT_LIBRARY: u32 = 1 << 6;

pub fn build_semantic_tokens(state: &DocumentState) -> Vec<u32> {
    let tokens = &state.tokens;
    let mut result = Vec::with_capacity(tokens.len() * 5);
    let mut prev_line: u32 = 0;
    let mut prev_col: u32 = 0;
    let declarations = modifiers::DeclarationIndex::build(state);

    for (i, tok) in tokens.iter().enumerate() {
        let colorable = tok.kind == TokenKind::Identifier
            || tok.kind.is_keyword()
            || matches!(tok.kind, TokenKind::Mut | TokenKind::Out)
            || tok.kind.is_literal()
            || tok.kind == TokenKind::RegularExpression
            || matches!(
                tok.kind,
                TokenKind::Arrow | TokenKind::FatArrow | TokenKind::PipeGt
            );
        if !colorable {
            continue;
        }

        let context = context::TokenContext::of(state, &declarations, i);
        let resolved = classify::resolve_token(state, tok, &context);
        let token_type = match resolved {
            Some(token_type) => token_type,
            None => match declarations.symbol_at(tok.offset) {
                Some(id) => classify::declared_tt(state, state.db.bind.arena.get(id)),
                None => continue,
            },
        };

        let (emit_col, emit_len) = match tok.kind {
            TokenKind::TemplateHead => (tok.col, tok.length.saturating_sub(2)),
            TokenKind::TemplateMiddle => (tok.col + 1, tok.length.saturating_sub(3)),
            TokenKind::TemplateTail => (tok.col + 1, tok.length.saturating_sub(1)),
            TokenKind::EOF
            | TokenKind::Dynamic
            | TokenKind::Identifier
            | TokenKind::IntegerLiteral
            | TokenKind::FloatLiteral
            | TokenKind::BinaryLiteral
            | TokenKind::OctalLiteral
            | TokenKind::HexLiteral
            | TokenKind::BigIntLiteral
            | TokenKind::Str
            | TokenKind::Char
            | TokenKind::Template
            | TokenKind::RegularExpression
            | TokenKind::LParen
            | TokenKind::RParen
            | TokenKind::LBrace
            | TokenKind::RBrace
            | TokenKind::LBracket
            | TokenKind::RBracket
            | TokenKind::LAngle
            | TokenKind::RAngle
            | TokenKind::Semicolon
            | TokenKind::Comma
            | TokenKind::Dot
            | TokenKind::DotDot
            | TokenKind::DotDotDot
            | TokenKind::DotDotEq
            | TokenKind::Colon
            | TokenKind::ColonColon
            | TokenKind::Question
            | TokenKind::QuestionDot
            | TokenKind::QuestionLBracket
            | TokenKind::QuestionQuestion
            | TokenKind::QuestionQuestionEq
            | TokenKind::Plus
            | TokenKind::PlusPlus
            | TokenKind::PlusEq
            | TokenKind::Minus
            | TokenKind::MinusMinus
            | TokenKind::MinusEq
            | TokenKind::Star
            | TokenKind::StarStar
            | TokenKind::StarEq
            | TokenKind::StarStarEq
            | TokenKind::Slash
            | TokenKind::SlashEq
            | TokenKind::Percent
            | TokenKind::PercentEq
            | TokenKind::Amp
            | TokenKind::AmpAmp
            | TokenKind::AmpEq
            | TokenKind::AmpAmpEq
            | TokenKind::Pipe
            | TokenKind::PipePipe
            | TokenKind::PipeEq
            | TokenKind::PipePipeEq
            | TokenKind::PipeGt
            | TokenKind::Caret
            | TokenKind::CaretEq
            | TokenKind::Tilde
            | TokenKind::LtLt
            | TokenKind::LtLtEq
            | TokenKind::GtGt
            | TokenKind::GtGtEq
            | TokenKind::GtGtGt
            | TokenKind::GtGtGtEq
            | TokenKind::Eq
            | TokenKind::EqEq
            | TokenKind::EqEqEq
            | TokenKind::Bang
            | TokenKind::BangEq
            | TokenKind::BangEqEq
            | TokenKind::Lt
            | TokenKind::LtEq
            | TokenKind::Gt
            | TokenKind::GtEq
            | TokenKind::Arrow
            | TokenKind::FatArrow
            | TokenKind::Let
            | TokenKind::Const
            | TokenKind::Var
            | TokenKind::Function
            | TokenKind::Class
            | TokenKind::Struct
            | TokenKind::Interface
            | TokenKind::Type
            | TokenKind::Enum
            | TokenKind::Namespace
            | TokenKind::Module
            | TokenKind::Extension
            | TokenKind::On
            | TokenKind::If
            | TokenKind::Else
            | TokenKind::Switch
            | TokenKind::Case
            | TokenKind::Default
            | TokenKind::While
            | TokenKind::For
            | TokenKind::Do
            | TokenKind::Break
            | TokenKind::Continue
            | TokenKind::Return
            | TokenKind::Throw
            | TokenKind::Try
            | TokenKind::Catch
            | TokenKind::Finally
            | TokenKind::Using
            | TokenKind::With
            | TokenKind::Import
            | TokenKind::Export
            | TokenKind::From
            | TokenKind::As
            | TokenKind::Async
            | TokenKind::Await
            | TokenKind::Yield
            | TokenKind::New
            | TokenKind::This
            | TokenKind::Super
            | TokenKind::Delete
            | TokenKind::Typeof
            | TokenKind::Instanceof
            | TokenKind::In
            | TokenKind::Of
            | TokenKind::Void
            | TokenKind::Is
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Null
            | TokenKind::Public
            | TokenKind::Private
            | TokenKind::Protected
            | TokenKind::Static
            | TokenKind::Abstract
            | TokenKind::Override
            | TokenKind::Readonly
            | TokenKind::Mut
            | TokenKind::Out
            | TokenKind::Declare
            | TokenKind::Native
            | TokenKind::Extends
            | TokenKind::Implements
            | TokenKind::Get
            | TokenKind::Set
            | TokenKind::Constructor
            | TokenKind::Destructor
            | TokenKind::Match
            | TokenKind::At
            | TokenKind::Hash
            | TokenKind::Backslash
            | TokenKind::Dollar
            | TokenKind::Backtick
            | TokenKind::Newline
            | TokenKind::Whitespace
            | TokenKind::DocComment
            | TokenKind::Placeholder
            | TokenKind::DecimalLiteral
            | TokenKind::Spawn
            | TokenKind::Parallel
            | TokenKind::Start
            | TokenKind::RawStr => (tok.col, tok.length),
        };
        if emit_len == 0 {
            continue;
        }

        let modifier = modifiers::modifiers_of(state, &declarations, i, tok);

        let delta_line = tok.line - prev_line;
        let delta_start = if delta_line == 0 {
            emit_col - prev_col
        } else {
            emit_col
        };

        result.push(delta_line);
        result.push(delta_start);
        result.push(emit_len);
        result.push(token_type);
        result.push(modifier);

        prev_line = tok.line;
        prev_col = emit_col;
    }

    result
}
