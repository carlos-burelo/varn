use crate::types::{CheckerTyTable, ClassMemberInfo, ClassMemberKind, ObjectTypeMember, Type};
use std::sync::Arc;
use varn_core::{Atom, AtomInterner, TypeKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Variance {
    Invariant,
    Covariant,
    Contravariant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pol {
    In,
    Out,
}

fn flip(p: Pol) -> Pol {
    match p {
        Pol::In => Pol::Out,
        Pol::Out => Pol::In,
    }
}

#[derive(Clone, Copy, Default)]
struct Uses {
    ins: bool,
    outs: bool,
}

struct Ctx<'a> {
    params: &'a [Arc<str>],
    uses: Vec<Uses>,
    table: &'a CheckerTyTable,
    interner: &'a AtomInterner,
}

fn resolve_name(ctx: &Ctx, atom: Atom) -> Option<String> {
    ctx.interner
        .try_resolve(atom)
        .or_else(|| ctx.table.name(atom))
        .map(|s| s.to_string())
}

fn mark(ctx: &mut Ctx, name: &str, pol: Pol, shadow: &[String]) {
    if shadow.iter().any(|s| s == name) {
        return;
    }
    if let Some(i) = ctx.params.iter().position(|p| p.as_ref() == name) {
        match pol {
            Pol::In => ctx.uses[i].ins = true,
            Pol::Out => ctx.uses[i].outs = true,
        }
    }
}

fn walk_both(ctx: &mut Ctx, id: crate::types::CheckerTyId, shadow: &[String]) {
    walk(ctx, id, Pol::In, shadow);
    walk(ctx, id, Pol::Out, shadow);
}

fn walk(ctx: &mut Ctx, id: crate::types::CheckerTyId, pol: Pol, shadow: &[String]) {
    let table = ctx.table;
    match table.get(id) {
        TypeKind::Named(n, _) => {
            if let Some(name) = resolve_name(ctx, n) {
                mark(ctx, &name, pol, shadow);
            }
        }
        TypeKind::Fn(fid) => {
            let ft = table.get_function(fid).clone();
            let mut shadow = shadow.to_vec();
            shadow.extend(ft.type_params.iter().map(|s| s.to_string()));
            for p in &ft.params {
                walk(ctx, p.ty, flip(pol), &shadow);
            }
            walk(ctx, ft.return_type, pol, &shadow);
        }
        TypeKind::Array(el) | TypeKind::KeyOf(el) => {
            walk_both(ctx, el, shadow);
        }
        TypeKind::Union(list)
        | TypeKind::Intersection(list)
        | TypeKind::Tuple(list)
        | TypeKind::TemplateLiteral(list) => {
            for id in table.get_list(list) {
                walk(ctx, *id, pol, shadow);
            }
        }
        TypeKind::Generic(_, args, _) => {
            for id in table.get_list(args) {
                walk_both(ctx, *id, shadow);
            }
        }
        TypeKind::Object(mid) => {
            for m in table.get_object_members(mid) {
                match m {
                    ObjectTypeMember::Property { ty, readonly, .. } => {
                        if *readonly {
                            walk(ctx, *ty, pol, shadow);
                        } else {
                            walk_both(ctx, *ty, shadow);
                        }
                    }
                    ObjectTypeMember::Method {
                        params,
                        return_type,
                        ..
                    } => {
                        for p in params {
                            walk(ctx, p.ty, flip(pol), shadow);
                        }
                        walk(ctx, *return_type, pol, shadow);
                    }
                    ObjectTypeMember::Index {
                        key_ty, value_ty, ..
                    } => {
                        walk_both(ctx, *key_ty, shadow);
                        walk_both(ctx, *value_ty, shadow);
                    }
                    ObjectTypeMember::Callable {
                        params,
                        return_type,
                        ..
                    } => {
                        for p in params {
                            walk(ctx, p.ty, flip(pol), shadow);
                        }
                        walk(ctx, *return_type, pol, shadow);
                    }
                }
            }
        }
        TypeKind::IndexedAccess { object, index } => {
            walk_both(ctx, object, shadow);
            walk_both(ctx, index, shadow);
        }
        TypeKind::Mapped {
            key_var,
            source,
            value,
            ..
        } => {
            let mut shadow = shadow.to_vec();
            if let Some(name) = resolve_name(ctx, key_var) {
                shadow.push(name);
            }
            walk_both(ctx, source, &shadow);
            walk_both(ctx, value, &shadow);
        }
        TypeKind::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            walk_both(ctx, check, shadow);
            walk_both(ctx, extends, shadow);
            walk(ctx, true_type, pol, shadow);
            walk(ctx, false_type, pol, shadow);
        }
        TypeKind::EnumVariant { payload_ty, .. } => {
            walk_both(ctx, payload_ty, shadow);
        }
        TypeKind::TypePredicate { target_type, .. } => {
            walk_both(ctx, target_type, shadow);
        }
        TypeKind::Primitive(_)
        | TypeKind::Builtin(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Typeof(_)
        | TypeKind::Infer(_) => {}
    }
}

pub fn measure_params(
    params: &[Arc<str>],
    members: &[ClassMemberInfo],
    bases: &[Type],
    table: &CheckerTyTable,
    interner: &AtomInterner,
) -> Vec<Variance> {
    let mut ctx = Ctx {
        params,
        uses: vec![Uses::default(); params.len()],
        table,
        interner,
    };
    for m in members {
        match m.kind {
            ClassMemberKind::Getter => walk(&mut ctx, m.ty.0, Pol::Out, &[]),
            ClassMemberKind::Setter => walk(&mut ctx, m.ty.0, Pol::In, &[]),
            ClassMemberKind::Method | ClassMemberKind::Function | ClassMemberKind::Constructor => {
                match table.get(m.ty.0) {
                    TypeKind::Fn(fid) => {
                        let ft = table.get_function(fid).clone();
                        let shadow: Vec<String> =
                            ft.type_params.iter().map(|s| s.to_string()).collect();
                        for p in &ft.params {
                            walk(&mut ctx, p.ty, Pol::In, &shadow);
                        }
                        walk(&mut ctx, ft.return_type, Pol::Out, &shadow);
                    }
                    TypeKind::Primitive(_)
                    | TypeKind::Builtin(_)
                    | TypeKind::Literal(_)
                    | TypeKind::This
                    | TypeKind::Array(_)
                    | TypeKind::Union(_)
                    | TypeKind::Intersection(_)
                    | TypeKind::Tuple(_)
                    | TypeKind::Named(..)
                    | TypeKind::Generic(..)
                    | TypeKind::TemplateLiteral(_)
                    | TypeKind::Object(_)
                    | TypeKind::Typeof(_)
                    | TypeKind::KeyOf(_)
                    | TypeKind::IndexedAccess { .. }
                    | TypeKind::Mapped { .. }
                    | TypeKind::Conditional { .. }
                    | TypeKind::Infer(_)
                    | TypeKind::EnumVariant { .. }
                    | TypeKind::TypePredicate { .. } => walk_both(&mut ctx, m.ty.0, &[]),
                }
            }
            ClassMemberKind::Property
            | ClassMemberKind::Variable
            | ClassMemberKind::Class
            | ClassMemberKind::Interface
            | ClassMemberKind::Namespace
            | ClassMemberKind::Enum
            | ClassMemberKind::Struct => {
                if m.is_readonly {
                    walk(&mut ctx, m.ty.0, Pol::Out, &[]);
                } else {
                    walk_both(&mut ctx, m.ty.0, &[]);
                }
            }
        }
    }
    for t in bases {
        walk_both(&mut ctx, t.0, &[]);
    }
    ctx.uses
        .iter()
        .map(|u| match (u.ins, u.outs) {
            (true, true) => Variance::Invariant,
            (true, false) => Variance::Contravariant,
            _ => Variance::Covariant,
        })
        .collect()
}
