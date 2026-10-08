use crate::expressions::{parse_call_args_pub, parse_expr};
use crate::stream::TokenStream;
use crate::types::parse_type;
use varn_core::ast::operators::{Modifiers, Visibility};
use varn_core::ast::{ArrayPatternEl, Decorator, ExprId, ExprKind, ObjPatternProp, Param, Pattern};
use varn_core::TokenKind;

pub fn parse_params(s: &mut TokenStream) -> Result<Vec<Param>, String> {
    s.expect(TokenKind::LParen)?;
    let mut params = vec![];
    while !s.check(TokenKind::RParen) && !s.is_eof() {
        params.push(parse_param(s)?);
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    s.expect(TokenKind::RParen)?;
    Ok(params)
}

pub fn parse_single_param(s: &mut TokenStream) -> Result<Param, String> {
    parse_param(s)
}

fn parse_param(s: &mut TokenStream) -> Result<Param, String> {
    let range = s.range();
    let mut mods = Modifiers::default();

    loop {
        match s.kind() {
            TokenKind::Public => {
                mods.visibility = Some(Visibility::Public);
                s.advance();
            }
            TokenKind::Private => {
                mods.visibility = Some(Visibility::Private);
                s.advance();
            }
            TokenKind::Protected => {
                mods.visibility = Some(Visibility::Protected);
                s.advance();
            }
            TokenKind::Readonly => {
                mods.is_readonly = true;
                s.advance();
            }
            TokenKind::EOF | TokenKind::Dynamic | TokenKind::Identifier | TokenKind::IntegerLiteral | TokenKind::FloatLiteral | TokenKind::BinaryLiteral | TokenKind::OctalLiteral | TokenKind::HexLiteral | TokenKind::BigIntLiteral | TokenKind::Str | TokenKind::Char | TokenKind::Template | TokenKind::TemplateHead | TokenKind::TemplateMiddle | TokenKind::TemplateTail | TokenKind::RegularExpression | TokenKind::LParen | TokenKind::RParen | TokenKind::LBrace | TokenKind::RBrace | TokenKind::LBracket | TokenKind::RBracket | TokenKind::LAngle | TokenKind::RAngle | TokenKind::Semicolon | TokenKind::Comma | TokenKind::Dot | TokenKind::DotDot | TokenKind::DotDotDot | TokenKind::DotDotEq | TokenKind::Colon | TokenKind::ColonColon | TokenKind::Question | TokenKind::QuestionDot | TokenKind::QuestionLBracket | TokenKind::QuestionQuestion | TokenKind::QuestionQuestionEq | TokenKind::Plus | TokenKind::PlusPlus | TokenKind::PlusEq | TokenKind::Minus | TokenKind::MinusMinus | TokenKind::MinusEq | TokenKind::Star | TokenKind::StarStar | TokenKind::StarEq | TokenKind::StarStarEq | TokenKind::Slash | TokenKind::SlashEq | TokenKind::Percent | TokenKind::PercentEq | TokenKind::Amp | TokenKind::AmpAmp | TokenKind::AmpEq | TokenKind::AmpAmpEq | TokenKind::Pipe | TokenKind::PipePipe | TokenKind::PipeEq | TokenKind::PipePipeEq | TokenKind::PipeGt | TokenKind::Caret | TokenKind::CaretEq | TokenKind::Tilde | TokenKind::LtLt | TokenKind::LtLtEq | TokenKind::GtGt | TokenKind::GtGtEq | TokenKind::GtGtGt | TokenKind::GtGtGtEq | TokenKind::Eq | TokenKind::EqEq | TokenKind::EqEqEq | TokenKind::Bang | TokenKind::BangEq | TokenKind::BangEqEq | TokenKind::Lt | TokenKind::LtEq | TokenKind::Gt | TokenKind::GtEq | TokenKind::Arrow | TokenKind::FatArrow | TokenKind::Let | TokenKind::Const | TokenKind::Var | TokenKind::Function | TokenKind::Class | TokenKind::Struct | TokenKind::Interface | TokenKind::Type | TokenKind::Enum | TokenKind::Namespace | TokenKind::Module | TokenKind::Extension | TokenKind::On | TokenKind::If | TokenKind::Else | TokenKind::Switch | TokenKind::Case | TokenKind::Default | TokenKind::While | TokenKind::For | TokenKind::Do | TokenKind::Break | TokenKind::Continue | TokenKind::Return | TokenKind::Throw | TokenKind::Try | TokenKind::Catch | TokenKind::Finally | TokenKind::Using | TokenKind::With | TokenKind::Import | TokenKind::Export | TokenKind::From | TokenKind::As | TokenKind::Async | TokenKind::Await | TokenKind::Yield | TokenKind::New | TokenKind::This | TokenKind::Super | TokenKind::Delete | TokenKind::Typeof | TokenKind::Instanceof | TokenKind::In | TokenKind::Of | TokenKind::Void | TokenKind::Is | TokenKind::True | TokenKind::False | TokenKind::Null | TokenKind::Static | TokenKind::Abstract | TokenKind::Override | TokenKind::Declare | TokenKind::Native | TokenKind::Extends | TokenKind::Implements | TokenKind::Get | TokenKind::Set | TokenKind::Constructor | TokenKind::Destructor | TokenKind::Match | TokenKind::At | TokenKind::Hash | TokenKind::Backslash | TokenKind::Dollar | TokenKind::Backtick | TokenKind::Newline | TokenKind::Whitespace | TokenKind::DocComment | TokenKind::Placeholder | TokenKind::DecimalLiteral | TokenKind::Spawn | TokenKind::Parallel | TokenKind::Start | TokenKind::RawStr => break,
        }
    }

    let is_rest = s.eat(TokenKind::DotDotDot);
    let pattern = parse_pattern(s)?;
    let is_optional = s.eat(TokenKind::Question);
    let type_ann = if s.eat(TokenKind::Colon) {
        Some(parse_type(s)?)
    } else {
        None
    };
    let default = if s.eat(TokenKind::Eq) {
        Some(parse_expr(s)?)
    } else {
        None
    };

    let full_range = s.span_from(range);
    Ok(Param {
        pattern,
        type_ann,
        default,
        is_rest,
        is_optional,
        modifiers: mods,
        range: full_range,
    })
}

pub fn parse_pattern(s: &mut TokenStream) -> Result<Pattern, String> {
    let range = s.range();
    match s.kind() {
        TokenKind::LBracket => parse_array_pattern(s),
        TokenKind::LBrace => parse_object_pattern(s),
        TokenKind::DotDotDot => {
            s.advance();
            let inner = parse_pattern(s)?;
            let full_range = s.span_from(range);
            Ok(Pattern::Rest {
                argument: Box::new(inner),
                range: full_range,
            })
        }
        TokenKind::Placeholder => {
            s.advance();
            let full_range = s.span_from(range);
            Ok(Pattern::Identifier {
                name: s.interner.intern("_"),
                range: full_range,
            })
        }
        TokenKind::EOF | TokenKind::Dynamic | TokenKind::Identifier | TokenKind::IntegerLiteral | TokenKind::FloatLiteral | TokenKind::BinaryLiteral | TokenKind::OctalLiteral | TokenKind::HexLiteral | TokenKind::BigIntLiteral | TokenKind::Str | TokenKind::Char | TokenKind::Template | TokenKind::TemplateHead | TokenKind::TemplateMiddle | TokenKind::TemplateTail | TokenKind::RegularExpression | TokenKind::LParen | TokenKind::RParen | TokenKind::RBrace | TokenKind::RBracket | TokenKind::LAngle | TokenKind::RAngle | TokenKind::Semicolon | TokenKind::Comma | TokenKind::Dot | TokenKind::DotDot | TokenKind::DotDotEq | TokenKind::Colon | TokenKind::ColonColon | TokenKind::Question | TokenKind::QuestionDot | TokenKind::QuestionLBracket | TokenKind::QuestionQuestion | TokenKind::QuestionQuestionEq | TokenKind::Plus | TokenKind::PlusPlus | TokenKind::PlusEq | TokenKind::Minus | TokenKind::MinusMinus | TokenKind::MinusEq | TokenKind::Star | TokenKind::StarStar | TokenKind::StarEq | TokenKind::StarStarEq | TokenKind::Slash | TokenKind::SlashEq | TokenKind::Percent | TokenKind::PercentEq | TokenKind::Amp | TokenKind::AmpAmp | TokenKind::AmpEq | TokenKind::AmpAmpEq | TokenKind::Pipe | TokenKind::PipePipe | TokenKind::PipeEq | TokenKind::PipePipeEq | TokenKind::PipeGt | TokenKind::Caret | TokenKind::CaretEq | TokenKind::Tilde | TokenKind::LtLt | TokenKind::LtLtEq | TokenKind::GtGt | TokenKind::GtGtEq | TokenKind::GtGtGt | TokenKind::GtGtGtEq | TokenKind::Eq | TokenKind::EqEq | TokenKind::EqEqEq | TokenKind::Bang | TokenKind::BangEq | TokenKind::BangEqEq | TokenKind::Lt | TokenKind::LtEq | TokenKind::Gt | TokenKind::GtEq | TokenKind::Arrow | TokenKind::FatArrow | TokenKind::Let | TokenKind::Const | TokenKind::Var | TokenKind::Function | TokenKind::Class | TokenKind::Struct | TokenKind::Interface | TokenKind::Type | TokenKind::Enum | TokenKind::Namespace | TokenKind::Module | TokenKind::Extension | TokenKind::On | TokenKind::If | TokenKind::Else | TokenKind::Switch | TokenKind::Case | TokenKind::Default | TokenKind::While | TokenKind::For | TokenKind::Do | TokenKind::Break | TokenKind::Continue | TokenKind::Return | TokenKind::Throw | TokenKind::Try | TokenKind::Catch | TokenKind::Finally | TokenKind::Using | TokenKind::With | TokenKind::Import | TokenKind::Export | TokenKind::From | TokenKind::As | TokenKind::Async | TokenKind::Await | TokenKind::Yield | TokenKind::New | TokenKind::This | TokenKind::Super | TokenKind::Delete | TokenKind::Typeof | TokenKind::Instanceof | TokenKind::In | TokenKind::Of | TokenKind::Void | TokenKind::Is | TokenKind::True | TokenKind::False | TokenKind::Null | TokenKind::Public | TokenKind::Private | TokenKind::Protected | TokenKind::Static | TokenKind::Abstract | TokenKind::Override | TokenKind::Readonly | TokenKind::Declare | TokenKind::Native | TokenKind::Extends | TokenKind::Implements | TokenKind::Get | TokenKind::Set | TokenKind::Constructor | TokenKind::Destructor | TokenKind::Match | TokenKind::At | TokenKind::Hash | TokenKind::Backslash | TokenKind::Dollar | TokenKind::Backtick | TokenKind::Newline | TokenKind::Whitespace | TokenKind::DocComment | TokenKind::DecimalLiteral | TokenKind::Spawn | TokenKind::Parallel | TokenKind::Start | TokenKind::RawStr => {
            let name = s.consume_lexeme();
            let full_range = s.span_from(range);
            Ok(Pattern::Identifier {
                name,
                range: full_range,
            })
        }
    }
}

fn parse_array_pattern(s: &mut TokenStream) -> Result<Pattern, String> {
    let range = s.range();
    s.advance();
    let mut elements: Vec<Option<ArrayPatternEl>> = vec![];
    let mut rest = None;

    while !s.check(TokenKind::RBracket) && !s.is_eof() {
        if s.check(TokenKind::Comma) {
            return Err(format!(
                "array destructuring holes are not allowed at {}:{}; use `_` to discard a position",
                s.range().start.line,
                s.range().start.column
            ));
        }
        if s.check(TokenKind::DotDotDot) {
            s.advance();
            rest = Some(Box::new(parse_pattern(s)?));
            s.eat(TokenKind::Comma);
            break;
        }
        let mut pat = parse_pattern(s)?;
        if s.eat(TokenKind::Eq) {
            let default = parse_expr(s)?;
            let assign_range = s.span_from(*pat.range());
            pat = Pattern::Assignment {
                left: Box::new(pat),
                right: default,
                range: assign_range,
            };
        }
        elements.push(Some(ArrayPatternEl { pattern: pat }));
        s.eat(TokenKind::Comma);
    }
    s.expect(TokenKind::RBracket)?;
    let full_range = s.span_from(range);
    Ok(Pattern::Array {
        elements,
        rest,
        range: full_range,
    })
}

fn parse_object_pattern(s: &mut TokenStream) -> Result<Pattern, String> {
    let range = s.range();
    s.advance();
    let mut properties = vec![];
    let mut rest = None;

    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        if s.check(TokenKind::DotDotDot) {
            s.advance();
            rest = Some(Box::new(parse_pattern(s)?));
            s.eat(TokenKind::Comma);
            break;
        }
        let prop_range = s.range();
        let key = s.consume_lexeme();
        let (value, shorthand) = if s.eat(TokenKind::Colon) {
            (parse_pattern(s)?, false)
        } else if s.eat(TokenKind::As) {
            let alias_range = s.range();
            let alias = s.consume_lexeme();
            (
                Pattern::Identifier {
                    name: alias,
                    range: alias_range,
                },
                false,
            )
        } else {
            (
                Pattern::Identifier {
                    name: key,
                    range: prop_range,
                },
                true,
            )
        };
        let value = if s.eat(TokenKind::Eq) {
            let default = parse_expr(s)?;
            let assign_range = s.span_from(prop_range);
            Pattern::Assignment {
                left: Box::new(value),
                right: default,
                range: assign_range,
            }
        } else {
            value
        };
        let full_prop_range = s.span_from(prop_range);
        properties.push(ObjPatternProp {
            key,
            value,
            shorthand,
            range: full_prop_range,
        });
        s.eat(TokenKind::Comma);
    }
    s.expect(TokenKind::RBrace)?;
    let full_range = s.span_from(range);
    Ok(Pattern::Object {
        properties,
        rest,
        range: full_range,
    })
}

pub fn parse_decorator_list(s: &mut TokenStream) -> Result<Vec<Decorator>, String> {
    let mut decorators = vec![];
    while s.check(TokenKind::At) {
        let range = s.range();
        s.advance();
        let expr = parse_decorator_expr(s)?;
        let full_range = s.span_from(range);
        decorators.push(Decorator {
            expression: expr,
            range: full_range,
        });
    }
    Ok(decorators)
}

fn parse_decorator_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    let range = s.range();
    let name = s.expect_id()?;
    let mut expr = s.expr(range, ExprKind::Identifier { name });

    while s.eat(TokenKind::Dot) {
        let prop_range = s.range();
        let prop = s.expect_id()?;
        let start_range = s.expr_range(expr);
        let prop_expr = s.expr(prop_range, ExprKind::Identifier { name: prop });
        expr = s.expr(
            start_range.to(prop_range),
            ExprKind::Member {
                object: expr,
                property: prop_expr,
                computed: false,
                optional: false,
            },
        );
    }

    if s.check(TokenKind::LAngle) {
        return Err("decorators do not support generic type arguments".to_owned());
    }
    if s.check(TokenKind::LParen) {
        let (type_args, args, call_range) = parse_call_args_pub(s)?;
        let start_range = s.expr_range(expr);
        expr = s.expr(
            start_range.to(call_range),
            ExprKind::Call {
                callee: expr,
                type_args,
                args,
                optional: false,
            },
        );
    }

    Ok(expr)
}
