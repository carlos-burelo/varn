use rustc_hash::{FxHashMap, FxHashSet};

use crate::document::{token_lexeme, TokenRecord};
use varn_core::TokenKind;

pub fn collect_type_params(
    source: &str,
    tokens: &[TokenRecord],
) -> (FxHashMap<String, Vec<String>>, FxHashSet<String>) {
    let mut name_to_params: FxHashMap<String, Vec<String>> = FxHashMap::default();
    let mut all_names: FxHashSet<String> = FxHashSet::default();
    let n = tokens.len();

    let mut i = 0;
    while i < n {
        if tokens[i].kind == TokenKind::LAngle && i >= 2 {
            let prev = &tokens[i - 1];
            let prev2 = &tokens[i - 2];
            if prev.kind == TokenKind::Identifier
                && matches!(
                    prev2.kind,
                    TokenKind::Class | TokenKind::Interface | TokenKind::Type | TokenKind::Function
                )
            {
                let sym_name = token_lexeme(source, prev).to_owned();
                let params = collect_type_param_names(source, tokens, i);
                for p in &params {
                    all_names.insert(p.clone());
                }
                name_to_params.insert(sym_name, params);
            }
        }
        i += 1;
    }

    (name_to_params, all_names)
}

fn collect_type_param_names(
    source: &str,
    tokens: &[TokenRecord],
    langle_idx: usize,
) -> Vec<String> {
    let mut params = Vec::new();
    let mut depth = 1i32;
    let mut j = langle_idx + 1;

    while j < tokens.len() {
        match tokens[j].kind {
            TokenKind::LAngle => depth += 1,
            TokenKind::RAngle => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            TokenKind::Identifier if depth == 1 => {
                let prev_kind = tokens[j - 1].kind;
                if matches!(prev_kind, TokenKind::LAngle | TokenKind::Comma) {
                    params.push(token_lexeme(source, &tokens[j]).to_owned());
                }
            }
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
            | TokenKind::TemplateHead
            | TokenKind::TemplateMiddle
            | TokenKind::TemplateTail
            | TokenKind::RegularExpression
            | TokenKind::LParen
            | TokenKind::RParen
            | TokenKind::LBrace
            | TokenKind::RBrace
            | TokenKind::LBracket
            | TokenKind::RBracket
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
            | TokenKind::RawStr => {}
        }
        j += 1;
    }

    params
}
