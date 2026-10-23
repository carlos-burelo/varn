use crate::types::{CheckerTyTable, ClassMemberKind, Type, TypeContext};
use std::sync::Arc;
use varn_core::TypeKind;

pub fn qualified_type(
    name: &str,
    ctx: Option<&dyn TypeContext>,
    table: &mut CheckerTyTable,
) -> Option<Type> {
    let ctx = ctx?;
    let (root, rest) = name.split_once('.')?;
    let mut ty = ctx.resolve_symbol(root)?;
    let mut origin = ctx
        .symbol_origin(root)
        .and_then(|a| text(a, ctx, table))
        .or_else(|| ctx.source_file().map(Arc::from));
    for segment in rest.split('.') {
        let TypeKind::Named(scope, declared_in) = table.get(ty.0) else {
            return None;
        };
        let scope = text(scope, ctx, table)?;
        origin = declared_in.and_then(|a| text(a, ctx, table)).or(origin);
        let members = ctx.get_namespace_members(&scope, origin.as_deref())?;
        let member = members.into_iter().find(|m| m.name.as_ref() == segment)?;
        ty = match member.kind {
            ClassMemberKind::Class
            | ClassMemberKind::Enum
            | ClassMemberKind::Interface
            | ClassMemberKind::Struct
            | ClassMemberKind::Namespace => {
                Type::named_with_origin(format!("{scope}::{segment}"), origin.clone(), table)
            }
            ClassMemberKind::Property => member.ty,
            ClassMemberKind::Constructor
            | ClassMemberKind::Method
            | ClassMemberKind::Function
            | ClassMemberKind::Variable
            | ClassMemberKind::Getter
            | ClassMemberKind::Setter => return None,
        };
    }
    Some(ty)
}

fn text(atom: varn_core::Atom, ctx: &dyn TypeContext, table: &CheckerTyTable) -> Option<Arc<str>> {
    table
        .name(atom)
        .map(Arc::from)
        .or_else(|| {
            ctx.interner()
                .and_then(|i| i.try_resolve(atom))
                .map(Arc::from)
        })
        .or_else(|| ctx.atom_text(atom).map(Arc::from))
}
