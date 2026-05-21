use crate::expressions::{parse_expr, parse_new_callee_expr};
use crate::stream::TokenStream;
use crate::types::{parse_type, parse_type_args, parse_type_params};
use varn_core::ast::operators::{Modifiers, Visibility};
use varn_core::ast::{ClassDecl, ClassMember, Decorator};
use varn_core::TokenKind;

pub fn parse_class_decl(
    s: &mut TokenStream,
    decorators: Vec<Decorator>,
    is_declare: bool,
) -> Result<ClassDecl, String> {
    let range = s.range();
    let is_abstract = s.eat(TokenKind::Abstract);
    s.expect(TokenKind::Class)?;

    let (id, id_offset) = if s.check(TokenKind::Identifier) {
        let id_offset = s.token().range.start.offset;
        let id = s.consume_lexeme();
        (Some(id), id_offset)
    } else {
        (None, 0)
    };
    let type_params = if s.check(TokenKind::LAngle) {
        parse_type_params(s)?
    } else {
        vec![]
    };

    let super_class = if s.eat(TokenKind::Extends) {
        Some(parse_new_callee_expr(s)?)
    } else {
        None
    };
    let super_type_args = if s.check(TokenKind::LAngle) {
        parse_type_args(s)?
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
    let mut body = vec![];
    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        while s.eat(TokenKind::Semicolon) {}
        while s.check(TokenKind::DocComment) {
            s.advance();
        }
        if s.check(TokenKind::RBrace) {
            break;
        }
        body.push(parse_class_member(s, is_declare)?);
    }
    s.expect(TokenKind::RBrace)?;

    Ok(ClassDecl {
        id: id.map(|name| name.into()),
        ast_id: 0,
        id_offset,
        type_params,
        super_class,
        super_type_args,
        implements,
        body,
        modifiers: Modifiers {
            is_abstract,
            is_declare,
            ..Default::default()
        },
        decorators,
        doc: None,
        range,
    })
}

fn parse_class_member(s: &mut TokenStream, class_is_declare: bool) -> Result<ClassMember, String> {
    let range = s.range();
    let decorators = super::super::patterns::parse_decorator_list(s)?;

    let mut mods = Modifiers::default();
    loop {
        let kind = s.kind();

        if kind.is_keyword()
            || matches!(
                kind,
                TokenKind::Async | TokenKind::Readonly | TokenKind::Declare
            )
        {
            let nk = s.peek_kind(1);
            if nk == TokenKind::LParen || nk == TokenKind::Colon || nk == TokenKind::Semicolon {
                break;
            }
        }

        match kind {
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
            TokenKind::Static => {
                mods.is_static = true;
                s.advance();
            }
            TokenKind::Abstract => {
                mods.is_abstract = true;
                s.advance();
            }
            TokenKind::Override => {
                mods.is_override = true;
                s.advance();
            }
            TokenKind::Readonly => {
                mods.is_readonly = true;
                s.advance();
            }
            TokenKind::Declare => {
                mods.is_declare = true;
                s.advance();
            }
            TokenKind::Async => {
                mods.is_async = true;
                s.advance();
            }
            _ => break,
        }
    }

    let is_generator = s.eat(TokenKind::Star);
    mods.is_generator = is_generator;

    if mods.is_static && s.check(TokenKind::LBrace) {
        let body = super::super::stmts::parse_block(s)?;
        return Ok(ClassMember::StaticBlock { body, range });
    }

    if s.check(TokenKind::Constructor) {
        s.advance();
        let params = super::super::patterns::parse_params(s)?;
        let body = if class_is_declare {
            if s.check(TokenKind::LBrace) {
                return Err("declare constructor cannot have a body".to_owned());
            }
            s.eat_semicolon();
            varn_core::ast::Stmt::new_with_range(s.range(), varn_core::ast::StmtKind::Empty)
        } else {
            super::super::stmts::parse_block(s)?
        };
        return Ok(ClassMember::Constructor {
            params,
            body,
            range,
        });
    }
    if s.check(TokenKind::Destructor) {
        s.advance();
        let body = if class_is_declare {
            if s.check(TokenKind::LBrace) {
                return Err("declare destructor cannot have a body".to_owned());
            }
            s.eat_semicolon();
            varn_core::ast::Stmt::new_with_range(s.range(), varn_core::ast::StmtKind::Empty)
        } else {
            super::super::stmts::parse_block(s)?
        };
        return Ok(ClassMember::Destructor { body, range });
    }

    let is_get = s.check(TokenKind::Get) && {
        let nk = s.peek_kind(1);
        nk != TokenKind::LParen && nk != TokenKind::Semicolon && nk != TokenKind::Colon
    };
    let is_set = s.check(TokenKind::Set) && {
        let nk = s.peek_kind(1);
        nk != TokenKind::LParen && nk != TokenKind::Semicolon && nk != TokenKind::Colon
    };

    if is_get {
        s.advance();
        let key = member_key_name(s)?;
        s.expect(TokenKind::LParen)?;
        s.expect(TokenKind::RParen)?;
        let return_type = if s.eat(TokenKind::Colon) {
            Some(parse_type(s)?)
        } else {
            None
        };
        let body = if s.check(TokenKind::LBrace) {
            if class_is_declare {
                return Err("declare getter cannot have a body".to_owned());
            }
            Some(super::super::stmts::parse_block(s)?)
        } else {
            s.eat_semicolon();
            None
        };
        return Ok(ClassMember::Getter {
            key: key.into(),
            return_type,
            body,
            modifiers: mods,
            range,
        });
    }
    if is_set {
        s.advance();
        let key = member_key_name(s)?;
        s.expect(TokenKind::LParen)?;
        let param = super::super::patterns::parse_single_param(s)?;
        s.expect(TokenKind::RParen)?;
        let body = if s.check(TokenKind::LBrace) {
            if class_is_declare {
                return Err("declare setter cannot have a body".to_owned());
            }
            Some(super::super::stmts::parse_block(s)?)
        } else {
            s.eat_semicolon();
            None
        };
        return Ok(ClassMember::Setter {
            key: key.into(),
            param,
            body,
            modifiers: mods,
            range,
        });
    }

    let key = member_key_name(s)?;
    let type_params = if s.check(TokenKind::LAngle) {
        parse_type_params(s)?
    } else {
        vec![]
    };

    if s.check(TokenKind::LParen) {
        let params = super::super::patterns::parse_params(s)?;
        let return_type = if s.eat(TokenKind::Colon) {
            Some(parse_type(s)?)
        } else {
            None
        };
        let body = if s.check(TokenKind::LBrace) {
            if class_is_declare {
                return Err("declare method cannot have a body".to_owned());
            }
            Some(super::super::stmts::parse_block(s)?)
        } else {
            s.eat_semicolon();
            None
        };
        return Ok(ClassMember::Method {
            key: key.into(),
            type_params,
            params,
            return_type,
            body,
            modifiers: mods,
            decorators,
            range,
        });
    }

    let type_ann = if s.eat(TokenKind::Colon) {
        Some(parse_type(s)?)
    } else {
        None
    };
    let init = if s.eat(TokenKind::Eq) {
        if class_is_declare {
            return Err("declare property cannot have initializer".to_owned());
        }
        Some(parse_expr(s)?)
    } else {
        None
    };
    s.eat_semicolon();
    Ok(ClassMember::Property {
        key: key.into(),
        type_ann,
        init,
        modifiers: mods,
        decorators,
        range,
    })
}

pub(super) fn member_key_name(s: &mut TokenStream) -> Result<String, String> {
    match s.kind() {
        TokenKind::Identifier => Ok(s.consume_str()),
        TokenKind::Str => Ok(s.consume_str()),
        TokenKind::IntegerLiteral => Ok(s.consume_str()),
        TokenKind::Hash => {
            s.advance();
            Ok(format!("#{}", s.consume_str()))
        }
        _ if s.kind().can_be_identifier() || s.kind().is_keyword() => {
            Ok(s.consume_str())
        }
        _ => Err(format!("Expected class member name, got {:?}", s.kind())),
    }
}
