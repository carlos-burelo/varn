use crate::types::{CheckerTyTable, Type, TypeContext};
use varn_core::ast::TypeNode;
use varn_core::TypeKind;

use super::aliases::try_stdlib_generic_alias;
use super::contexts::AliasSubstitutionContext;
use super::names::resolve_atom_name;
use super::resolve_type_node;

pub(super) fn resolve_generic_type(
    name: varn_core::Atom,
    args: &[TypeNode],
    ctx: Option<&dyn TypeContext>,
    interner: &varn_core::AtomInterner,
    table: &mut CheckerTyTable,
) -> Type {
    let name_str = resolve_atom_name(name, ctx, interner);
    if name_str.as_ref() == varn_core::well_known::RECORD {
        return Type::Dynamic;
    }
    let resolved_args: Vec<Type> = args
        .iter()
        .map(|m| resolve_type_node(m, ctx, table))
        .collect();

    if let Some((params, alias_node)) = ctx.and_then(|c| c.get_alias_node(&name_str)) {
        if !params.is_empty() && params.len() == resolved_args.len() {
            let alias_ctx = AliasSubstitutionContext {
                inner: ctx,
                params,
                args: resolved_args,
            };
            return resolve_type_node(&alias_node, Some(&alias_ctx), table);
        }
    }

    if let Some(ty) = try_stdlib_generic_alias(&name_str, &resolved_args, ctx, table) {
        return ty;
    }

    if name_str.as_ref() == varn_core::BuiltinType::Array.name() {
        if let [el] = resolved_args.as_slice() {
            return Type::array(*el, table);
        }
    }

    let source_origin = ctx
        .and_then(|c| c.source_file())
        .map(|s| table.intern_name(s));
    let origin = ctx
        .and_then(|c| c.resolve_symbol(&name_str))
        .and_then(|t| match table.get(t.0) {
            TypeKind::Named(_, o) | TypeKind::Generic(_, _, o) => o,
            TypeKind::Primitive(_)
            | TypeKind::Builtin(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Object(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => None,
        })
        .or_else(|| ctx.and_then(|c| c.symbol_origin(&name_str)))
        .or(source_origin);
    Type::generic_atom(name, resolved_args, origin, table)
}

pub(super) fn resolve_named_type(
    name: varn_core::Atom,
    ctx: Option<&dyn TypeContext>,
    interner: &varn_core::AtomInterner,
    table: &mut CheckerTyTable,
) -> Type {
    use super::aliases::resolve_primitive;
    let name_str = resolve_atom_name(name, ctx, interner);
    let prim = resolve_primitive(&name_str, ctx, table);
    if !matches!(table.get(prim.0), TypeKind::Named(_, _)) {
        return prim;
    }

    if let Some((params, alias_node)) = ctx.and_then(|c| c.get_alias_node(&name_str)) {
        if params.is_empty() {
            return resolve_type_node(&alias_node, ctx, table);
        }
    }

    if let Some(resolved) = ctx.and_then(|c| c.resolve_symbol(&name_str)) {
        if let (TypeKind::Named(_, None), Some(origin)) = (
            table.get(resolved.0),
            ctx.and_then(|c| c.symbol_origin(&name_str)),
        ) {
            return resolved.with_origin(origin, table);
        }
        return resolved;
    }

    prim
}
