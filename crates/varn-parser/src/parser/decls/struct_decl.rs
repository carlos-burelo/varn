use super::class_member::{member_doc, parse_class_member};
use crate::stream::TokenStream;
use crate::types::{parse_type, parse_type_params};
use varn_core::ast::operators::Modifiers;
use varn_core::ast::{ClassDecl, ClassMember, Decorator, Param, Pattern, StmtKind};
use varn_core::TokenKind;

pub fn parse_struct_decl(
    s: &mut TokenStream,
    decorators: Vec<Decorator>,
) -> Result<ClassDecl, String> {
    let range = s.range();
    s.expect(TokenKind::Struct)?;

    let id_offset = s.token().range.start.offset;
    let id = s.expect_id()?;
    let type_params = if s.check(TokenKind::LAngle) {
        parse_type_params(s)?
    } else {
        vec![]
    };

    if s.check(TokenKind::Extends) {
        return Err(
            "a struct cannot extend another type: it is a value, not a hierarchy".to_owned(),
        );
    }
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
    let mut fields: Vec<Param> = vec![];
    let mut members: Vec<ClassMember> = vec![];
    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        while s.eat(TokenKind::Semicolon) || s.eat(TokenKind::Comma) {}
        let doc = member_doc(s);
        if s.check(TokenKind::RBrace) {
            break;
        }
        match parse_class_member(s, false, doc)? {
            ClassMember::Property {
                key,
                type_ann,
                init,
                modifiers,
                decorators,
                range: field_range,
                ..
            } if !modifiers.is_static => {
                if !decorators.is_empty() {
                    return Err("decorators are not supported on struct fields".to_owned());
                }
                if fields
                    .iter()
                    .any(|f| matches!(&f.pattern, Pattern::Identifier { name, .. } if *name == key))
                {
                    return Err(format!(
                        "the struct already has a field named '{}'",
                        s.interner.resolve(key)
                    ));
                }
                if type_ann.is_none() {
                    return Err(format!(
                        "the field '{}' of a struct needs a type: write `name: Type`",
                        s.interner.resolve(key)
                    ));
                }
                fields.push(Param {
                    pattern: Pattern::Identifier {
                        name: key,
                        range: field_range,
                    },
                    type_ann,
                    default: init,
                    is_rest: false,
                    is_optional: false,
                    modifiers: Modifiers {
                        visibility: modifiers.visibility,
                        is_readonly: true,
                        ..Default::default()
                    },
                    range: field_range,
                });
            }
            ClassMember::Constructor { .. } => {
                return Err(
                    "a struct has no constructor: its fields, in order, are its constructor"
                        .to_owned(),
                );
            }
            ClassMember::Destructor { .. } => {
                return Err("a struct has no destructor: it owns no resources".to_owned());
            }
            ClassMember::Setter { .. } => {
                return Err("a struct is immutable: it cannot declare a setter".to_owned());
            }
            member @ (ClassMember::Property { .. }
            | ClassMember::Method { .. }
            | ClassMember::Getter { .. }
            | ClassMember::StaticBlock { .. }) => members.push(member),
        }
    }
    s.expect(TokenKind::RBrace)?;

    let full_range = s.span_from(range);
    let empty_body = s.stmt(full_range, StmtKind::Block { stmts: vec![] });
    let mut body = Vec::with_capacity(members.len() + 1);
    body.push(ClassMember::Constructor {
        params: fields,
        body: empty_body,
        decorators: vec![],
        doc: None,
        range: full_range,
    });
    body.extend(members);

    Ok(ClassDecl {
        id: Some(id),
        ast_id: s.next_ast_id(),
        id_offset,
        type_params,
        primary_params: None,
        super_class: None,
        super_type_args: vec![],
        implements,
        body,
        modifiers: Modifiers::default(),
        decorators,
        doc: None,
        range: full_range,
        is_struct: true,
    })
}
