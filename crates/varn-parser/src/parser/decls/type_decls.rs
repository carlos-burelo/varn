use super::class::member_key_name;
use crate::expressions::parse_expr;
use crate::stream::TokenStream;
use crate::types::{parse_type, parse_type_params};
use std::rc::Rc;
use varn_core::ast::decl::{Decl, StructField};
use varn_core::ast::{
    EnumDecl, EnumField, EnumMember, InterfaceDecl, InterfaceMember, NamespaceDecl, StmtKind,
    StructDecl, SumField, SumTypeDecl, SumVariant, TypeAliasDecl,
};
use varn_core::TokenKind;

pub fn parse_interface_decl(s: &mut TokenStream) -> Result<InterfaceDecl, String> {
    let range = s.range();
    s.expect(TokenKind::Interface)?;
    let id = s.expect_id()?;
    let type_params = if s.check(TokenKind::LAngle) {
        parse_type_params(s)?
    } else {
        vec![]
    };
    let extends = if s.eat(TokenKind::Extends) {
        let mut e = vec![parse_type(s)?];
        while s.eat(TokenKind::Comma) {
            e.push(parse_type(s)?);
        }
        e
    } else {
        vec![]
    };

    s.expect(TokenKind::LBrace)?;
    let mut body = vec![];
    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        s.eat(TokenKind::Semicolon);
        if s.check(TokenKind::RBrace) {
            break;
        }
        body.push(parse_interface_member(s)?);
    }
    s.expect(TokenKind::RBrace)?;

    let full_range = s.span_from(range);
    Ok(InterfaceDecl {
        id,
        ast_id: s.next_ast_id(),
        type_params,
        extends,
        body,
        doc: None,
        range: full_range,
    })
}

pub fn parse_interface_member(s: &mut TokenStream) -> Result<InterfaceMember, String> {
    let mem_range = s.range();
    let readonly = s.eat(TokenKind::Readonly);
    let is_async = s.check(TokenKind::Async)
        && !matches!(
            s.peek_kind(1),
            TokenKind::LParen | TokenKind::Colon | TokenKind::Question | TokenKind::LAngle
        )
        && s.eat(TokenKind::Async);

    if s.check(TokenKind::LBracket) {
        s.advance();
        let param = super::super::patterns::parse_single_param(s)?;
        s.expect(TokenKind::RBracket)?;
        s.expect(TokenKind::Colon)?;
        let return_type = parse_type(s)?;
        s.eat(TokenKind::Semicolon);
        let full_range = s.span_from(mem_range);
        return Ok(InterfaceMember::Index {
            param,
            return_type,
            range: full_range,
        });
    }
    if s.check(TokenKind::LParen) {
        let params = super::super::patterns::parse_params(s)?;
        s.expect(TokenKind::Colon)?;
        let return_type = parse_type(s)?;
        s.eat(TokenKind::Semicolon);
        let full_range = s.span_from(mem_range);
        return Ok(InterfaceMember::Callable {
            params,
            return_type,
            range: full_range,
        });
    }

    let key = member_key_name(s)?;
    let optional = s.eat(TokenKind::Question);

    if s.check(TokenKind::LParen) || s.check(TokenKind::LAngle) {
        let type_params = if s.check(TokenKind::LAngle) {
            parse_type_params(s)?
        } else {
            vec![]
        };
        let params = super::super::patterns::parse_params(s)?;
        let return_type = if s.eat(TokenKind::Colon) {
            Some(parse_type(s)?)
        } else {
            None
        };
        s.eat_semicolon();
        let full_range = s.span_from(mem_range);
        return Ok(InterfaceMember::Method {
            key: key.into(),
            type_params,
            params,
            return_type,
            optional,
            is_async,
            range: full_range,
        });
    }

    let type_ann = if s.eat(TokenKind::Colon) {
        parse_type(s)?
    } else {
        s.type_node(
            s.range(),
            varn_core::TypeKind::Intrinsic(varn_core::TypeTag::Dynamic),
        )
    };
    s.eat_semicolon();
    let full_range = s.span_from(mem_range);
    Ok(InterfaceMember::Property {
        key: key.into(),
        type_ann,
        optional,
        readonly,
        range: full_range,
    })
}

pub fn parse_sum_type_or_alias(s: &mut TokenStream) -> Result<Decl, String> {
    let range = s.range();
    s.expect(TokenKind::Type)?;
    let id = s.expect_id()?;

    let type_params = if s.check(TokenKind::LAngle) {
        parse_type_params(s)?
    } else {
        vec![]
    };

    s.expect(TokenKind::Eq)?;

    if s.check(TokenKind::Pipe) {
        let decl = parse_sum_type_body(id, type_params, range, s)?;
        return Ok(Decl::SumType(decl));
    }

    let alias = parse_type(s)?;
    s.eat_semicolon();
    let full_range = s.span_from(range);
    Ok(Decl::TypeAlias(TypeAliasDecl {
        id,
        ast_id: s.next_ast_id(),
        type_params,
        alias,
        doc: None,
        range: full_range,
    }))
}

fn parse_sum_type_body(
    id: Rc<str>,
    type_params: Vec<varn_core::ast::TypeParam>,
    range: varn_core::source::SourceRange,
    s: &mut TokenStream,
) -> Result<SumTypeDecl, String> {
    let mut variants = Vec::new();

    while s.check(TokenKind::Pipe) {
        let v_start = s.range();
        s.advance();
        let vname = s.expect_id()?;

        let mut fields = Vec::new();
        if s.check(TokenKind::LParen) {
            s.advance();
            while !s.check(TokenKind::RParen) && !s.is_eof() {
                let fname = s.expect_id()?;
                s.expect(TokenKind::Colon)?;
                let fty = parse_type(s)?;
                fields.push(SumField {
                    name: fname,
                    ty: fty,
                });
                if s.check(TokenKind::Comma) {
                    s.advance();
                }
            }
            s.expect(TokenKind::RParen)?;
        }

        let vrange = s.span_from(v_start);
        variants.push(SumVariant {
            name: vname,
            fields,
            range: vrange,
        });
    }

    let full_range = s.span_from(range);
    Ok(SumTypeDecl {
        id,
        ast_id: s.next_ast_id(),
        type_params,
        variants,
        doc: None,
        range: full_range,
    })
}

pub fn parse_enum_decl(s: &mut TokenStream) -> Result<EnumDecl, String> {
    let range = s.range();
    s.expect(TokenKind::Enum)?;
    let id = s.expect_id()?;

    let type_params = if s.check(TokenKind::LAngle) {
        parse_type_params(s)?
    } else {
        vec![]
    };

    let implements = if s.eat(TokenKind::Implements) {
        let mut impls = vec![parse_type(s)?];
        while s.eat(TokenKind::Comma) {
            impls.push(parse_type(s)?);
        }
        impls
    } else {
        vec![]
    };

    s.expect(TokenKind::LBrace)?;
    let mut members = vec![];
    let mut body = vec![];

    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        if s.eat(TokenKind::Semicolon) {
            break;
        }

        let mem_range = s.range();
        let name = s.expect_id()?;

        let mut payload_fields: Vec<EnumField> = Vec::new();
        if s.check(TokenKind::LParen) {
            s.advance();
            let mut idx = 0;
            while !s.check(TokenKind::RParen) && !s.is_eof() {
                let field_range = s.range();
                let field_name =
                    if s.check(TokenKind::Identifier) && s.peek_kind(1) == TokenKind::Colon {
                        let name = s.consume_lexeme();
                        s.expect(TokenKind::Colon)?;
                        name
                    } else {
                        let n = Rc::from(format!("value{idx}").as_str());
                        idx += 1;
                        n
                    };
                let (ty, init) = match s.kind() {
                    TokenKind::IntegerLiteral => {
                        let expr = crate::expressions::parse_expr(s)?;
                        (
                            s.type_node(
                                field_range,
                                varn_core::TypeKind::Intrinsic(varn_core::TypeTag::Int),
                            ),
                            Some(expr),
                        )
                    }
                    TokenKind::FloatLiteral => {
                        let expr = crate::expressions::parse_expr(s)?;
                        (
                            s.type_node(
                                field_range,
                                varn_core::TypeKind::Intrinsic(varn_core::TypeTag::Float),
                            ),
                            Some(expr),
                        )
                    }
                    TokenKind::Str => {
                        let expr = crate::expressions::parse_expr(s)?;
                        (
                            s.type_node(
                                field_range,
                                varn_core::TypeKind::Intrinsic(varn_core::TypeTag::Str),
                            ),
                            Some(expr),
                        )
                    }
                    TokenKind::True | TokenKind::False => {
                        let expr = crate::expressions::parse_expr(s)?;
                        (
                            s.type_node(
                                field_range,
                                varn_core::TypeKind::Intrinsic(varn_core::TypeTag::Bool),
                            ),
                            Some(expr),
                        )
                    }
                    _ => {
                        let parsed_ty = crate::types::parse_type(s)?;
                        (parsed_ty, None)
                    }
                };

                let f_full_range = s.span_from(field_range);
                payload_fields.push(EnumField {
                    name: field_name,
                    ty,
                    init,
                    range: f_full_range,
                });
                if s.check(TokenKind::Comma) {
                    s.advance();
                }
            }
            s.expect(TokenKind::RParen)?;
        }

        let init = if payload_fields.is_empty() && s.eat(TokenKind::Eq) {
            Some(parse_expr(s)?)
        } else {
            None
        };

        let full_mem_range = s.span_from(mem_range);
        members.push(EnumMember {
            id: name,
            init,
            payload_fields,
            range: full_mem_range,
        });

        let has_comma = s.eat(TokenKind::Comma);
        if s.check(TokenKind::Semicolon) {
            continue;
        }
        if !has_comma && !s.check(TokenKind::Identifier) && !s.check(TokenKind::Semicolon) {
            break;
        }
    }

    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        while s.eat(TokenKind::Semicolon) {}
        while s.check(TokenKind::DocComment) {
            s.advance();
        }
        if s.check(TokenKind::RBrace) {
            break;
        }
        body.push(super::class::parse_class_member(s, false)?);
    }

    s.expect(TokenKind::RBrace)?;
    let full_range = s.span_from(range);
    Ok(EnumDecl {
        id,
        ast_id: s.next_ast_id(),
        type_params,
        implements,
        members,
        body,
        doc: None,
        range: full_range,
    })
}

pub fn parse_namespace_decl(s: &mut TokenStream) -> Result<NamespaceDecl, String> {
    let range = s.range();
    s.advance();
    let id = s.expect_id()?;
    s.expect(TokenKind::LBrace)?;
    let mut body = vec![];
    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        while s.eat(TokenKind::Semicolon) {}
        if s.check(TokenKind::RBrace) {
            break;
        }
        let stmt = super::super::stmts::parse_stmt_or_decl_inner(s)?;
        if let StmtKind::Decl(d) = stmt.kind {
            body.push(*d);
        }
    }
    s.expect(TokenKind::RBrace)?;
    let full_range = s.span_from(range);
    Ok(NamespaceDecl {
        id,
        ast_id: s.next_ast_id(),
        body,
        doc: None,
        range: full_range,
    })
}

pub fn parse_struct_decl(s: &mut TokenStream) -> Result<StructDecl, String> {
    let range = s.range();
    s.expect(TokenKind::Struct)?;
    let id = s.expect_id()?;
    s.expect(TokenKind::LBrace)?;
    let mut fields = vec![];
    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        while s.eat(TokenKind::Semicolon) {}
        if s.check(TokenKind::RBrace) {
            break;
        }
        let field_range = s.range();
        let name = s.expect_id()?;
        s.expect(TokenKind::Colon)?;
        let type_ann = parse_type(s)?;
        let default = if s.eat(TokenKind::Eq) {
            Some(parse_expr(s)?)
        } else {
            None
        };
        let full_field_range = s.span_from(field_range);
        fields.push(StructField {
            name,
            type_ann,
            default,
            range: full_field_range,
        });
        s.eat(TokenKind::Comma);
    }
    s.expect(TokenKind::RBrace)?;
    let full_range = s.span_from(range);
    Ok(StructDecl {
        id,
        ast_id: s.next_ast_id(),
        fields,
        doc: None,
        range: full_range,
    })
}
