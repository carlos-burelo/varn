use crate::types::{CheckerTyTable, ObjectTypeMember, Type, TypeContext};
use varn_core::ast::TypeNode;
use varn_core::TypeKind;

use super::resolve_type_node;

pub(super) fn resolve_intersection_type(
    members: &[TypeNode],
    ctx: Option<&dyn TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    let resolved: Vec<Type> = members
        .iter()
        .map(|m| resolve_type_node(m, ctx, table))
        .collect();

    let scalars: Vec<Type> = resolved
        .iter()
        .copied()
        .filter(|m| is_scalar(m, table))
        .collect();
    if let Some((&first, rest)) = scalars.split_first() {
        return rest
            .iter()
            .fold(first, |acc, m| intersect_two(acc, *m, table));
    }

    let parts_opt: Option<Vec<Vec<ObjectTypeMember>>> = resolved
        .iter()
        .map(|m| match table.get(m.0) {
            TypeKind::Object(mid) => Some(table.get_object_members(mid).to_vec()),
            TypeKind::Named(name, origin) => {
                let ctx = ctx?;
                let name_str = ctx.atom_text(name)?;
                let origin_str = origin.and_then(|o| ctx.atom_text(o));
                let members = ctx
                    .get_class_members(&name_str, origin_str.as_deref())
                    .or_else(|| ctx.get_interface_members(&name_str, origin_str.as_deref()))?;
                Some(
                    members
                        .iter()
                        .map(|cm| cm.as_object_member(table))
                        .collect(),
                )
            }
            _ => None,
        })
        .collect();

    if let Some(parts) = parts_opt {
        return Type::object(merge_members(parts.into_iter().flatten(), table), table);
    }
    let ids: Vec<crate::types::CheckerTyId> = resolved.iter().map(|t| t.0).collect();
    let list = table.intern_list(&ids);
    Type::resolved(table.intern(TypeKind::Intersection(list)))
}

fn is_scalar(ty: &Type, table: &CheckerTyTable) -> bool {
    matches!(
        table.get(ty.0),
        TypeKind::Primitive(_) | TypeKind::Literal(_)
    ) && *ty != Type::Dynamic
}

fn intersect_two(a: Type, b: Type, table: &mut CheckerTyTable) -> Type {
    if a == b || b == Type::Dynamic {
        return a;
    }
    if a == Type::Dynamic {
        return b;
    }
    if a == Type::Never || b == Type::Never {
        return Type::Never;
    }
    match (table.get(a.0), table.get(b.0)) {
        (TypeKind::Literal(l), TypeKind::Primitive(p)) if l.base() == p => a,
        (TypeKind::Primitive(p), TypeKind::Literal(l)) if l.base() == p => b,
        (
            TypeKind::Primitive(_) | TypeKind::Literal(_),
            TypeKind::Primitive(_) | TypeKind::Literal(_),
        ) => Type::Never,
        _ => {
            let list = table.intern_list(&[a.0, b.0]);
            Type::resolved(table.intern(TypeKind::Intersection(list)))
        }
    }
}

fn merge_members(
    members: impl Iterator<Item = ObjectTypeMember>,
    table: &mut CheckerTyTable,
) -> Vec<ObjectTypeMember> {
    let mut out: Vec<ObjectTypeMember> = Vec::new();
    for member in members {
        let ObjectTypeMember::Property {
            name,
            ty,
            optional,
            readonly,
        } = &member
        else {
            out.push(member);
            continue;
        };
        let existing = out
            .iter_mut()
            .find(|m| matches!(m, ObjectTypeMember::Property { name: n, .. } if n == name));
        match existing {
            Some(ObjectTypeMember::Property {
                ty: prev_ty,
                optional: prev_opt,
                readonly: prev_ro,
                ..
            }) => {
                *prev_ty = intersect_two(Type::resolved(*prev_ty), Type::resolved(*ty), table).0;
                *prev_opt = *prev_opt && *optional;
                *prev_ro = *prev_ro || *readonly;
            }
            _ => out.push(member),
        }
    }
    out
}
