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
        let end = s.range();
        return Ok(TypeNode {
            id: 0,
            kind: TypeKind::Conditional {
                check: Box::new(ty),
                extends: Box::new(extends_ty),
                true_type: Box::new(true_ty),
                false_type: Box::new(false_ty),
            },
            range: start.to(end),
        });
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
    let end = s.range();
    Ok(TypeNode {
        id: 0,
        kind: TypeKind::Union(members),
        range: start.to(end),
    })
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
    let end = s.range();
    Ok(TypeNode {
        id: 0,
        kind: TypeKind::Intersection(members),
        range: start.to(end),
    })
}

fn is_ternary_at(s: &TokenStream) -> bool {
    if s.kind() != TokenKind::Question {
        return false;
    }
    let start_line = s.line();
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
            let end_range = s.range();
            s.advance();
            ty = TypeNode {
                id: 0,
                kind: TypeKind::Array(Box::new(ty)),
                range: start.to(end_range),
            };
        } else if s.check(TokenKind::LBracket) && s.peek_kind(1) != TokenKind::RBracket {
            s.advance();
            let index = parse_type(s)?;
            let end_range = s.range();
            s.expect(TokenKind::RBracket)?;
            ty = TypeNode {
                id: 0,
                kind: TypeKind::IndexedAccess {
                    object: Box::new(ty),
                    index: Box::new(index),
                },
                range: start.to(end_range),
            };
        } else if s.check(TokenKind::Question) && !is_ternary_at(s) {
            s.advance();
            let end_range = s.range();
            ty = TypeNode {
                id: 0,
                kind: TypeKind::Union(vec![
                    ty,
                    TypeNode {
                        id: 0,
                        kind: TypeKind::Intrinsic(TypeTag::Null),
                        range: end_range,
                    },
                ]),
                range: start.to(end_range),
            };
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
                let end = s.range();
                return Ok(TypeNode {
                    id: 0,
                    kind: TypeKind::KeyOf(Box::new(inner)),
                    range: range.to(end),
                });
            }

            if s.lexeme() == "infer" && s.peek_kind(1) == TokenKind::Identifier {
                s.advance();
                let name = s.consume_lexeme();
                let end = s.range();
                return Ok(TypeNode {
                    id: 0,
                    kind: TypeKind::Infer(name.to_string()),
                    range: range.to(end),
                });
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
            let end = s.range();
            let kind = if type_args.is_empty() {
                TypeKind::Named(name, None)
            } else {
                TypeKind::Generic(name, type_args, None)
            };
            Ok(TypeNode {
                id: 0,
                kind,
                range: range.to(end),
            })
        }

        TokenKind::This => {
            s.advance();
            Ok(TypeNode {
                id: 0,
                kind: TypeKind::This,
                range,
            })
        }

        TokenKind::Typeof => {
            s.advance();
            let expr = crate::expressions::parse_unary_expr(s)?;
            let end = s.range();
            Ok(TypeNode {
                id: 0,
                kind: TypeKind::Typeof(Box::new(expr)),
                range: range.to(end),
            })
        }

        TokenKind::LParen => {
            s.advance();

            if s.check(TokenKind::RParen) {
                s.advance();
                if s.eat(TokenKind::FatArrow) {
                    let ret = parse_type(s)?;
                    let end = s.range();
                    return Ok(TypeNode {
                        id: 0,
                        kind: TypeKind::Fn((vec![], Box::new(ret))),
                        range: range.to(end),
                    });
                }
                return Err(format!(
                    "expected '=>' after '()' in type position at {}:{}",
                    s.line(),
                    s.column()
                ));
            }

            if (s.kind() == TokenKind::Identifier && s.peek_kind(1) == TokenKind::Colon)
                || s.check(TokenKind::DotDotDot)
            {
                let params = parse_fn_type_params(s)?;
                s.expect(TokenKind::RParen)?;
                s.expect(TokenKind::FatArrow)?;
                let ret = parse_type(s)?;
                let end = s.range();
                return Ok(TypeNode {
                    id: 0,
                    kind: TypeKind::Fn((params, Box::new(ret))),
                    range: range.to(end),
                });
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
                let end = s.range();
                let params = param_types
                    .into_iter()
                    .map(|ty| TypeParam {
                        name: "_".to_string(),
                        constraint: Some(ty),
                        default: None,
                        range,
                    })
                    .collect();
                return Ok(TypeNode {
                    id: 0,
                    kind: TypeKind::Fn((params, Box::new(ret))),
                    range: range.to(end),
                });
            }

            s.expect(TokenKind::RParen)?;
            if s.eat(TokenKind::FatArrow) {
                let ret = parse_type(s)?;
                let end = s.range();
                return Ok(TypeNode {
                    id: 0,
                    kind: TypeKind::Fn((
                        vec![TypeParam {
                            name: "_".to_string(),
                            constraint: Some(first),
                            default: None,
                            range,
                        }],
                        Box::new(ret),
                    )),
                    range: range.to(end),
                });
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
            let end = s.range();
            s.expect(TokenKind::RBracket)?;
            Ok(TypeNode {
                id: 0,
                kind: TypeKind::Tuple(elements),
                range: range.to(end),
            })
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
                let end = s.range();
                s.expect(TokenKind::RBrace)?;
                return Ok(TypeNode {
                    id: 0,
                    kind: TypeKind::Mapped {
                        key_var: key_var.to_string(),
                        source: Box::new(source),
                        value: Box::new(value),
                        optional,
                        readonly: mapped_readonly,
                    },
                    range: range.to(end),
                });
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
            let end = s.range();
            s.expect(TokenKind::RBrace)?;
            Ok(TypeNode {
                id: 0,
                kind: TypeKind::Object(members),
                range: range.to(end),
            })
        }

        TokenKind::Str => {
            let value = s.consume_lexeme();
            Ok(TypeNode {
                id: 0,
                kind: TypeKind::LiteralStr(value.to_string()),
                range,
            })
        }

        TokenKind::IntegerLiteral => {
            let pre_parsed = s.parsed_num();
            let value = s.lexeme().to_owned();
            s.advance();
            let int_val = match pre_parsed {
                Some(varn_core::ParsedNumber::Int(n)) => n,
                _ => value.parse().unwrap_or(0),
            };
            Ok(TypeNode {
                id: 0,
                kind: TypeKind::LiteralInt(int_val),
                range,
            })
        }
        TokenKind::FloatLiteral => {
            let pre_parsed = s.parsed_num();
            let value = s.lexeme().to_owned();
            s.advance();
            let float_val = match pre_parsed {
                Some(varn_core::ParsedNumber::Float(f)) => f,
                _ => value.parse::<f64>().unwrap_or(0.0),
            };
            Ok(TypeNode {
                id: 0,
                kind: TypeKind::LiteralFloat(float_val.to_bits()),
                range,
            })
        }
        TokenKind::True => {
            s.advance();
            Ok(TypeNode {
                id: 0,
                kind: TypeKind::LiteralBool(true),
                range,
            })
        }
        TokenKind::False => {
            s.advance();
            Ok(TypeNode {
                id: 0,
                kind: TypeKind::LiteralBool(false),
                range,
            })
        }
        TokenKind::Null => {
            s.advance();
            Ok(TypeNode {
                id: 0,
                kind: TypeKind::Intrinsic(TypeTag::Null),
                range,
            })
        }

        TokenKind::Void => {
            s.advance();
            Ok(TypeNode {
                id: 0,
                kind: TypeKind::Intrinsic(TypeTag::Void),
                range,
            })
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
            Ok(TypeNode {
                id: 0,
                kind: TypeKind::Named(name.to_string(), None),
                range,
            })
        }

        _ => Err(format!(
            "Unexpected token in type position: {:?} at {}:{}",
            s.kind(),
            s.line(),
            s.column()
        )),
    }
}

fn parse_template_literal_type(s: &mut TokenStream) -> Result<TypeNode, String> {
    let start = s.range();
    let mut parts: Vec<TypeNode> = vec![];

    let raw = s.consume_lexeme();
    let literal_text = raw.trim_start_matches('`');
    let (head_text, has_interp) = if let Some(text) = literal_text.strip_suffix("${") {
        (text, true)
    } else {
        (literal_text.trim_end_matches('`'), false)
    };
    parts.push(TypeNode {
        id: 0,
        kind: TypeKind::LiteralStr(head_text.to_string()),
        range: start,
    });

    if !has_interp {
        return Ok(TypeNode {
            id: 0,
            kind: TypeKind::LiteralStr(head_text.to_string()),
            range: start,
        });
    }

    loop {
        let interp_ty = parse_type(s)?;
        parts.push(interp_ty);

        if !matches!(
            s.kind(),
            TokenKind::TemplateMiddle | TokenKind::TemplateTail
        ) {
            return Err(format!(
                "expected template continuation in type literal at {}:{}",
                s.line(),
                s.column()
            ));
        }

        let cont_range = s.range();
        let raw_cont = s.consume_lexeme();
        let (content, is_tail) = if let Some(text) = raw_cont.strip_suffix('`') {
            (text.strip_prefix('}').unwrap_or(text), true)
        } else {
            let after_close = raw_cont.strip_prefix('}').unwrap_or(raw_cont.as_ref());
            (after_close.trim_end_matches("${"), false)
        };
        parts.push(TypeNode {
            id: 0,
            kind: TypeKind::LiteralStr(content.to_string()),
            range: cont_range,
        });

        if is_tail {
            let end = s.range();
            return Ok(TypeNode {
                id: 0,
                kind: TypeKind::TemplateLiteral(parts),
                range: start.to(end),
            });
        }
    }
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
    s.expect(TokenKind::RAngle)?;
    Ok(args)
}

pub fn parse_type_params(s: &mut TokenStream) -> Result<Vec<TypeParam>, String> {
    s.expect(TokenKind::LAngle)?;
    let mut params = vec![];
    while !s.check(TokenKind::RAngle) && !s.is_eof() {
        let range = s.range();
        let name_str = s.lexeme().to_owned();
        let _name_tok = s.expect_token(TokenKind::Identifier)?;
        let name = varn_core::intern_string(&name_str);
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
        let end = s.range();
        params.push(TypeParam {
            name: name.to_string(),
            constraint,
            default,
            range: range.to(end),
        });
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    s.expect(TokenKind::RAngle)?;
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
        let end = s.range();
        params.push(TypeParam {
            name: name.to_string(),
            constraint: Some(ty),
            default: None,
            range: prange.to(end),
        });
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    Ok(params)
}
