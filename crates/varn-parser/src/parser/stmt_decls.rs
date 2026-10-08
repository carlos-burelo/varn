use crate::stream::TokenStream;
use varn_core::ast::{Decl, StmtId, StmtKind};
use varn_core::TokenKind;

pub(super) fn try_parse_decl_stmt(
    s: &mut TokenStream,
    kind: TokenKind,
    next_kind: TokenKind,
    decorators: Vec<varn_core::ast::Decorator>,
) -> Option<Result<StmtId, String>> {
    if kind == TokenKind::Declare {
        s.advance();
        return try_parse_decl_stmt_mode(s, s.kind(), s.peek_kind(1), decorators, true);
    }

    try_parse_decl_stmt_mode(s, kind, next_kind, decorators, false)
}

pub(super) fn try_parse_decl_stmt_mode(
    s: &mut TokenStream,
    kind: TokenKind,
    next_kind: TokenKind,
    decorators: Vec<varn_core::ast::Decorator>,
    is_declare: bool,
) -> Option<Result<StmtId, String>> {
    let result = match kind {
        TokenKind::Function => {
            let mut decl = match super::decls::parse_function_decl(s, decorators, false, is_declare)
            {
                Ok(decl) => decl,
                Err(err) => return Some(Err(err)),
            };
            decl.doc = s.current_doc();
            let range = decl.range;
            Ok(s.stmt(range, StmtKind::Decl(Box::new(Decl::Function(decl)))))
        }
        TokenKind::Async if next_kind == TokenKind::Function => {
            s.advance();
            let mut decl = match super::decls::parse_function_decl(s, decorators, true, is_declare)
            {
                Ok(decl) => decl,
                Err(err) => return Some(Err(err)),
            };
            decl.doc = s.current_doc();
            let range = decl.range;
            Ok(s.stmt(range, StmtKind::Decl(Box::new(Decl::Function(decl)))))
        }
        TokenKind::Class | TokenKind::Abstract => {
            let mut decl = match super::decls::parse_class_decl(s, decorators, is_declare) {
                Ok(decl) => decl,
                Err(err) => return Some(Err(err)),
            };
            decl.doc = s.current_doc();
            let range = decl.range;
            Ok(s.stmt(range, StmtKind::Decl(Box::new(Decl::Class(decl)))))
        }
        TokenKind::Interface => {
            if !decorators.is_empty() {
                return Some(Err("decorators are not supported on interfaces".to_owned()));
            }
            let mut decl = match super::decls::parse_interface_decl(s) {
                Ok(decl) => decl,
                Err(err) => return Some(Err(err)),
            };
            decl.doc = s.current_doc();
            let range = decl.range;
            Ok(s.stmt(range, StmtKind::Decl(Box::new(Decl::Interface(decl)))))
        }
        TokenKind::Type => {
            if !decorators.is_empty() {
                return Some(Err(
                    "decorators are not supported on type aliases".to_owned()
                ));
            }
            let mut decl = match super::decls::parse_type_alias_decl(s) {
                Ok(decl) => decl,
                Err(err) => return Some(Err(err)),
            };
            if let Decl::TypeAlias(d) = &mut decl {
                d.doc = s.current_doc();
            }
            let range = *decl.range();
            Ok(s.stmt(range, StmtKind::Decl(Box::new(decl))))
        }
        TokenKind::Enum => {
            if !decorators.is_empty() {
                return Some(Err("decorators are not supported on enums".to_owned()));
            }
            let mut decl = match super::decls::parse_enum_decl(s) {
                Ok(decl) => decl,
                Err(err) => return Some(Err(err)),
            };
            decl.doc = s.current_doc();
            let range = decl.range;
            Ok(s.stmt(range, StmtKind::Decl(Box::new(Decl::Enum(decl)))))
        }
        TokenKind::Namespace | TokenKind::Module => {
            if !decorators.is_empty() {
                return Some(Err("decorators are not supported on namespaces".to_owned()));
            }
            let mut decl = match super::decls::parse_namespace_decl(s) {
                Ok(decl) => decl,
                Err(err) => return Some(Err(err)),
            };
            decl.doc = s.current_doc();
            let range = decl.range;
            Ok(s.stmt(range, StmtKind::Decl(Box::new(Decl::Namespace(decl)))))
        }
        TokenKind::Struct => {
            if !decorators.is_empty() {
                return Some(Err("decorators are not supported on structs".to_owned()));
            }
            let mut decl = match super::decls::parse_struct_decl(s) {
                Ok(decl) => decl,
                Err(err) => return Some(Err(err)),
            };
            decl.doc = s.current_doc();
            let range = decl.range;
            Ok(s.stmt(range, StmtKind::Decl(Box::new(Decl::Struct(decl)))))
        }
        TokenKind::Extension => {
            if !decorators.is_empty() {
                return Some(Err("decorators are not supported on extensions".to_owned()));
            }
            let decl = match super::decls::parse_extension_decl(s) {
                Ok(decl) => decl,
                Err(err) => return Some(Err(err)),
            };
            let range = decl.range;
            Ok(s.stmt(range, StmtKind::Decl(Box::new(Decl::Extension(decl)))))
        }
        TokenKind::Let | TokenKind::Const | TokenKind::Var => {
            if !decorators.is_empty() {
                return Some(Err(
                    "decorators are not supported on variable declarations".to_owned()
                ));
            }
            let mut decl = match super::decls::parse_var_decl_with_declare(s, is_declare) {
                Ok(decl) => decl,
                Err(err) => return Some(Err(err)),
            };
            decl.doc = s.current_doc();
            s.eat_semicolon();
            let range = decl.range;
            Ok(s.stmt(range, StmtKind::Decl(Box::new(Decl::Variable(decl)))))
        }
        TokenKind::Import => {
            if !decorators.is_empty() {
                return Some(Err("decorators are not supported on imports".to_owned()));
            }
            let decl = match super::decls::parse_import_decl(s) {
                Ok(decl) => decl,
                Err(err) => return Some(Err(err)),
            };
            let _ = s.current_doc();
            s.eat_semicolon();
            let range = decl.range;
            Ok(s.stmt(range, StmtKind::Decl(Box::new(Decl::Import(decl)))))
        }
        TokenKind::Export => {
            let decl = match super::decls::parse_export_decl(s, decorators) {
                Ok(decl) => decl,
                Err(err) => return Some(Err(err)),
            };
            let _ = s.current_doc();
            s.eat_semicolon();
            let range = *decl.range();
            Ok(s.stmt(range, StmtKind::Decl(Box::new(Decl::Export(decl)))))
        }
        TokenKind::EOF | TokenKind::Dynamic | TokenKind::Identifier | TokenKind::IntegerLiteral | TokenKind::FloatLiteral | TokenKind::BinaryLiteral | TokenKind::OctalLiteral | TokenKind::HexLiteral | TokenKind::BigIntLiteral | TokenKind::Str | TokenKind::Char | TokenKind::Template | TokenKind::TemplateHead | TokenKind::TemplateMiddle | TokenKind::TemplateTail | TokenKind::RegularExpression | TokenKind::LParen | TokenKind::RParen | TokenKind::LBrace | TokenKind::RBrace | TokenKind::LBracket | TokenKind::RBracket | TokenKind::LAngle | TokenKind::RAngle | TokenKind::Semicolon | TokenKind::Comma | TokenKind::Dot | TokenKind::DotDot | TokenKind::DotDotDot | TokenKind::DotDotEq | TokenKind::Colon | TokenKind::ColonColon | TokenKind::Question | TokenKind::QuestionDot | TokenKind::QuestionLBracket | TokenKind::QuestionQuestion | TokenKind::QuestionQuestionEq | TokenKind::Plus | TokenKind::PlusPlus | TokenKind::PlusEq | TokenKind::Minus | TokenKind::MinusMinus | TokenKind::MinusEq | TokenKind::Star | TokenKind::StarStar | TokenKind::StarEq | TokenKind::StarStarEq | TokenKind::Slash | TokenKind::SlashEq | TokenKind::Percent | TokenKind::PercentEq | TokenKind::Amp | TokenKind::AmpAmp | TokenKind::AmpEq | TokenKind::AmpAmpEq | TokenKind::Pipe | TokenKind::PipePipe | TokenKind::PipeEq | TokenKind::PipePipeEq | TokenKind::PipeGt | TokenKind::Caret | TokenKind::CaretEq | TokenKind::Tilde | TokenKind::LtLt | TokenKind::LtLtEq | TokenKind::GtGt | TokenKind::GtGtEq | TokenKind::GtGtGt | TokenKind::GtGtGtEq | TokenKind::Eq | TokenKind::EqEq | TokenKind::EqEqEq | TokenKind::Bang | TokenKind::BangEq | TokenKind::BangEqEq | TokenKind::Lt | TokenKind::LtEq | TokenKind::Gt | TokenKind::GtEq | TokenKind::Arrow | TokenKind::FatArrow | TokenKind::On | TokenKind::If | TokenKind::Else | TokenKind::Switch | TokenKind::Case | TokenKind::Default | TokenKind::While | TokenKind::For | TokenKind::Do | TokenKind::Break | TokenKind::Continue | TokenKind::Return | TokenKind::Throw | TokenKind::Try | TokenKind::Catch | TokenKind::Finally | TokenKind::Using | TokenKind::With | TokenKind::From | TokenKind::As | TokenKind::Async | TokenKind::Await | TokenKind::Yield | TokenKind::New | TokenKind::This | TokenKind::Super | TokenKind::Delete | TokenKind::Typeof | TokenKind::Instanceof | TokenKind::In | TokenKind::Of | TokenKind::Void | TokenKind::Is | TokenKind::True | TokenKind::False | TokenKind::Null | TokenKind::Public | TokenKind::Private | TokenKind::Protected | TokenKind::Static | TokenKind::Override | TokenKind::Readonly | TokenKind::Declare | TokenKind::Native | TokenKind::Extends | TokenKind::Implements | TokenKind::Get | TokenKind::Set | TokenKind::Constructor | TokenKind::Destructor | TokenKind::Match | TokenKind::At | TokenKind::Hash | TokenKind::Backslash | TokenKind::Dollar | TokenKind::Backtick | TokenKind::Newline | TokenKind::Whitespace | TokenKind::DocComment | TokenKind::Placeholder | TokenKind::DecimalLiteral | TokenKind::Spawn | TokenKind::Parallel | TokenKind::Start | TokenKind::RawStr => return None,
    };

    Some(result)
}
