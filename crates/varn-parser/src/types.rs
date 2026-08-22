use crate::stream::TokenStream;
use std::rc::Rc;
use varn_core::ast::{TypeNode, TypeParam};
use varn_core::{TokenKind, TypeKind, TypeTag};

pub fn parse_type(s: &mut TokenStream) -> Result<TypeNode, String> {
    let start = s.range();
    let ty = parse_union_type(s)?;

    if s.eat(TokenKind::Extends) {
        let extends_ty = parse_union_type(s)?;
        s.expect(TokenKind::Question)?;
        let true_ty = parse_type(s)?;
        s.expect(TokenKind::Colon)?;
        let false_ty = parse_type(s)?;
        let full_range = s.span_from(start);
        return Ok(s.type_node(
            full_range,
            TypeKind::Conditional {
                check: Box::new(ty),
                extends: Box::new(extends_ty),
                true_type: Box::new(true_ty),
                false_type: Box::new(false_ty),
            },
        ));
    }

    Ok(ty)
}

fn parse_union_type(s: &mut TokenStream) -> Result<TypeNode, String> {
    let start = s.range();
    let first = parse_intersection_type(s)?;

    if !s.check(TokenKind::Pipe) {
        return Ok(first);
    }

    let mut members = vec![first];
    while s.eat(TokenKind::Pipe) {
        members.push(parse_intersection_type(s)?);
    }
    let full_range = s.span_from(start);
    Ok(s.type_node(full_range, TypeKind::Union(members)))
}

fn parse_intersection_type(s: &mut TokenStream) -> Result<TypeNode, String> {
    let start = s.range();
    let first = parse_array_type(s)?;

    if !s.check(TokenKind::Amp) {
        return Ok(first);
    }

    let mut members = vec![first];
    while s.eat(TokenKind::Amp) {
        members.push(parse_array_type(s)?);
    }
    let full_range = s.span_from(start);
    Ok(s.type_node(full_range, TypeKind::Intersection(members)))
}

fn is_ternary_at(s: &TokenStream) -> bool {
    if s.kind() != TokenKind::Question {
        return false;
    }
    let start_line = s.peek_line(0);
    let mut off = 1;
    let next = s.peek_kind(off);
    if next == TokenKind::Colon {
        return false;
    }
    loop {
        if s.peek_line(off) != start_line {
            return false;
        }
        let kind = s.peek_kind(off);
        if kind == TokenKind::Colon {
            return true;
        }
        if kind == TokenKind::Semicolon
            || kind == TokenKind::Comma
            || kind == TokenKind::RParen
            || kind == TokenKind::RBracket
            || kind == TokenKind::RBrace
            || kind == TokenKind::EOF
            || kind == TokenKind::Question
            || kind == TokenKind::Eq
            || kind.starts_statement()
        {
            return false;
        }
        off += 1;
    }
}

fn parse_array_type(s: &mut TokenStream) -> Result<TypeNode, String> {
    let start = s.range();
    let mut ty = parse_primary_type(s)?;

    loop {
        if s.check(TokenKind::LBracket) && s.peek_kind(1) == TokenKind::RBracket {
            s.advance();
            s.advance();
            let full_range = s.span_from(start);
            ty = s.type_node(full_range, TypeKind::Array(Box::new(ty)));
        } else if s.check(TokenKind::LBracket) && s.peek_kind(1) != TokenKind::RBracket {
            s.advance();
            let index = parse_type(s)?;
            s.expect(TokenKind::RBracket)?;
            let full_range = s.span_from(start);
            ty = s.type_node(
                full_range,
                TypeKind::IndexedAccess {
                    object: Box::new(ty),
                    index: Box::new(index),
                },
            );
        } else if s.check(TokenKind::Question) && !is_ternary_at(s) {
            let q_range = s.range();
            s.advance();
            let full_range = s.span_from(start);
            let null_node = s.type_node(q_range, TypeKind::Intrinsic(TypeTag::Null));
            ty = s.type_node(full_range, TypeKind::Union(vec![ty, null_node]));
        } else {
            break;
        }
    }
    Ok(ty)
}

fn parse_primary_type(s: &mut TokenStream) -> Result<TypeNode, String> {
    let range = s.range();

    match s.kind() {
        TokenKind::Template | TokenKind::TemplateHead => parse_template_literal_type(s),

        TokenKind::Identifier => {
            if s.lexeme() == "keyof" {
                s.advance();
                let inner = parse_array_type(s)?;
                let full_range = s.span_from(range);
                return Ok(s.type_node(full_range, TypeKind::KeyOf(Box::new(inner))));
            }

            if s.lexeme() == "infer" && s.peek_kind(1) == TokenKind::Identifier {
                s.advance();
                let name = s.consume_lexeme();
                let full_range = s.span_from(range);
                return Ok(s.type_node(full_range, TypeKind::Infer(name.to_string())));
            }

            let mut name_buf = s.lexeme().to_owned();
            s.advance();

            while s.check(TokenKind::Dot) {
                s.advance();
                if s.check(TokenKind::Identifier) {
                    name_buf.push('.');
                    name_buf.push_str(s.lexeme());
                    s.advance();
                } else {
                    break;
                }
            }
            let name = name_buf.to_string();

            let type_args = if s.check(TokenKind::LAngle) {
                parse_type_args(s)?
            } else {
                vec![]
            };
            let full_range = s.span_from(range);
            let kind = if type_args.is_empty() {
                TypeKind::Named(name, None)
            } else {
                TypeKind::Generic(name, type_args, None)
            };
            Ok(s.type_node(full_range, kind))
        }

        TokenKind::This => {
            s.advance();
            Ok(s.type_node(range, TypeKind::This))
        }

        TokenKind::Typeof => {
            s.advance();
            let expr = crate::expressions::parse_unary_expr(s)?;
            let full_range = s.span_from(range);
            Ok(s.type_node(full_range, TypeKind::Typeof(Box::new(expr))))
        }

        TokenKind::LParen => {
            s.advance();

            if s.check(TokenKind::RParen) {
                s.advance();
                if s.eat(TokenKind::FatArrow) {
                    let ret = parse_type(s)?;
                    let full_range = s.span_from(range);
                    return Ok(s.type_node(full_range, TypeKind::Fn((vec![], Box::new(ret)))));
                }
                return Err(format!(
                    "expected '=>' after '()' in type position at {}:{}",
                    s.range().start.line,
                    s.range().start.column
                ));
            }

            if (s.kind() == TokenKind::Identifier && s.peek_kind(1) == TokenKind::Colon)
                || s.check(TokenKind::DotDotDot)
            {
                let params = parse_fn_type_params(s)?;
                s.expect(TokenKind::RParen)?;
                s.expect(TokenKind::FatArrow)?;
                let ret = parse_type(s)?;
                let full_range = s.span_from(range);
                return Ok(s.type_node(full_range, TypeKind::Fn((params, Box::new(ret)))));
            }

            let first = parse_type(s)?;
            if s.eat(TokenKind::Comma) {
                let mut param_types = vec![first];
                while !s.check(TokenKind::RParen) && !s.is_eof() {
                    param_types.push(parse_type(s)?);
                    if !s.eat(TokenKind::Comma) {
                        break;
                    }
                }
                s.expect(TokenKind::RParen)?;
                s.expect(TokenKind::FatArrow)?;
                let ret = parse_type(s)?;
                let full_range = s.span_from(range);
                let params = param_types
                    .into_iter()
                    .map(|ty| {
                        let ty_range = *ty.range();
                        TypeParam {
                            name: "_".to_string(),
                            constraint: Some(ty),
                            default: None,
                            range: ty_range,
                        }
                    })
                    .collect();
                return Ok(s.type_node(full_range, TypeKind::Fn((params, Box::new(ret)))));
            }

            s.expect(TokenKind::RParen)?;
            if s.eat(TokenKind::FatArrow) {
                let ret = parse_type(s)?;
                let full_range = s.span_from(range);
                let first_range = *first.range();
                return Ok(s.type_node(
                    full_range,
                    TypeKind::Fn((
                        vec![TypeParam {
                            name: "_".to_string(),
                            constraint: Some(first),
                            default: None,
                            range: first_range,
                        }],
                        Box::new(ret),
                    )),
                ));
            }

            Ok(first)
        }

        TokenKind::LBracket => {
            s.advance();
            let mut elements = vec![];
            while !s.check(TokenKind::RBracket) && !s.is_eof() {
                elements.push(parse_type(s)?);
                if !s.eat(TokenKind::Comma) {
                    break;
                }
            }
            s.expect(TokenKind::RBracket)?;
            let full_range = s.span_from(range);
            Ok(s.type_node(full_range, TypeKind::Tuple(elements)))
        }

        TokenKind::LBrace => {
            s.advance();

            let is_mapped = if s.check(TokenKind::Readonly) {
                s.peek_kind(1) == TokenKind::LBracket
                    && s.peek_kind(2) == TokenKind::Identifier
                    && s.peek_kind(3) == TokenKind::In
            } else {
                s.check(TokenKind::LBracket)
                    && s.peek_kind(1) == TokenKind::Identifier
                    && s.peek_kind(2) == TokenKind::In
            };

            if is_mapped {
                let mapped_readonly = s.eat(TokenKind::Readonly);
                s.advance();
                let key_var = s.consume_lexeme();
                s.advance();
                let source = parse_type(s)?;
                s.expect(TokenKind::RBracket)?;
                let optional = s.eat(TokenKind::Question);
                s.expect(TokenKind::Colon)?;
                let value = parse_type(s)?;
                s.expect(TokenKind::RBrace)?;
                let full_range = s.span_from(range);
                return Ok(s.type_node(
                    full_range,
                    TypeKind::Mapped {
                        key_var: key_var.to_string(),
                        source: Box::new(source),
                        value: Box::new(value),
                        optional,
                        readonly: mapped_readonly,
                    },
                ));
            }

            let mut members = vec![];
            while !s.check(TokenKind::RBrace) && !s.is_eof() {
                while s.eat(TokenKind::Semicolon) || s.eat(TokenKind::Comma) {}
                if s.check(TokenKind::RBrace) {
                    break;
                }
                members.push(crate::parser::decls::type_decls::parse_interface_member(s)?);

                s.eat(TokenKind::Comma);
                s.eat(TokenKind::Semicolon);
            }
            s.expect(TokenKind::RBrace)?;
            let full_range = s.span_from(range);
            Ok(s.type_node(full_range, TypeKind::Object(members)))
        }

        TokenKind::Str
        | TokenKind::IntegerLiteral
        | TokenKind::FloatLiteral
        | TokenKind::True
        | TokenKind::False => Err(format!(
            "literal types are not supported; use primitive types (int, float, str, bool) at {}:{}",
            s.range().start.line,
            s.range().start.column
        )),
        TokenKind::Null => {
            s.advance();
            Ok(s.type_node(range, TypeKind::Intrinsic(TypeTag::Null)))
        }

        TokenKind::Void => {
            s.advance();
            Ok(s.type_node(range, TypeKind::Intrinsic(TypeTag::Void)))
        }

        TokenKind::Is
        | TokenKind::On
        | TokenKind::Get
        | TokenKind::Set
        | TokenKind::From
        | TokenKind::Of
        | TokenKind::Async
        | TokenKind::Static
        | TokenKind::Abstract
        | TokenKind::Readonly
        | TokenKind::Native
        | TokenKind::Constructor
        | TokenKind::Destructor => {
            let name = s.consume_lexeme();
            Ok(s.type_node(range, TypeKind::Named(name.to_string(), None)))
        }

        _ => Err(format!(
            "Unexpected token in type position: {:?} at {}:{}",
            s.kind(),
            s.range().start.line,
            s.range().start.column
        )),
    }
}

fn parse_template_literal_type(s: &mut TokenStream) -> Result<TypeNode, String> {
    let start = s.range();
    let raw = s.consume_lexeme();
    let has_interp = raw.ends_with("${");
    if has_interp {
        loop {
            let _ = parse_type(s)?;
            if !matches!(
                s.kind(),
                TokenKind::TemplateMiddle | TokenKind::TemplateTail
            ) {
                return Err(format!(
                    "expected template continuation in type position at {}:{}",
                    s.range().start.line,
                    s.range().start.column
                ));
            }
            let raw_cont = s.consume_lexeme();
            if raw_cont.ends_with('`') {
                break;
            }
        }
    }
    let full_range = s.span_from(start);
    Ok(s.type_node(full_range, TypeKind::Intrinsic(varn_core::TypeTag::Str)))
}

pub fn parse_type_args(s: &mut TokenStream) -> Result<Vec<TypeNode>, String> {
    s.expect(TokenKind::LAngle)?;
    let mut args = vec![];
    while !s.check(TokenKind::RAngle) && !s.is_eof() {
        args.push(parse_type(s)?);
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    s.expect_rangle()?;
    Ok(args)
}

pub fn parse_type_params(s: &mut TokenStream) -> Result<Vec<TypeParam>, String> {
    s.expect(TokenKind::LAngle)?;
    let mut params = vec![];
    while !s.check(TokenKind::RAngle) && !s.is_eof() {
        let range = s.range();
        let name = s.expect_id()?;
        let constraint = if s.eat(TokenKind::Extends) {
            Some(parse_union_type(s)?)
        } else {
            None
        };
        let default = if s.eat(TokenKind::Eq) {
            Some(parse_type(s)?)
        } else {
            None
        };
        let full_range = s.span_from(range);
        params.push(TypeParam {
            name: name.to_string(),
            constraint,
            default,
            range: full_range,
        });
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    s.expect_rangle()?;
    Ok(params)
}

fn parse_fn_type_params(s: &mut TokenStream) -> Result<Vec<TypeParam>, String> {
    let mut params = vec![];
    while !s.check(TokenKind::RParen) && !s.is_eof() {
        let prange = s.range();

        s.eat(TokenKind::DotDotDot);

        let name = if s.kind() == TokenKind::Identifier && s.peek_kind(1) == TokenKind::Colon {
            let n = s.consume_lexeme();
            s.advance();
            n
        } else {
            Rc::from("_")
        };
        let ty = parse_type(s)?;
        let full_range = s.span_from(prange);
        params.push(TypeParam {
            name: name.to_string(),
            constraint: Some(ty),
            default: None,
            range: full_range,
        });
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    Ok(params)
}
