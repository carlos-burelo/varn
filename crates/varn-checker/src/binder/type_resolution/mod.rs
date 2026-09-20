mod aliases;
mod conditional;
mod contexts;
mod keyed_access;
mod mapped;
mod template;

use crate::types::{CheckerTyTable, FunctionType, ObjectTypeMember, Type, TypeContext};
use std::rc::Rc;
use varn_core::ast::TypeNode;
use varn_core::TypeKind;

use aliases::{is_primitive_type, try_stdlib_generic_alias};
use conditional::resolve_conditional;
use contexts::AliasSubstitutionContext;
use keyed_access::{resolve_indexed_access, resolve_keyof};
use mapped::resolve_mapped;
use template::resolve_template_literal_type;

pub use aliases::resolve_primitive;

pub fn resolve_type_node(
    node: &TypeNode,
    ctx: Option<&dyn TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    use varn_core::TypeTag;
    // `node.kind`'s name slots (`TypeKind::Named`/`Generic`/interface member
    // keys) are `Atom` at the AST layer; resolving them to the `&str` this
    // function's checker-`Type` output and the `TypeContext` lookups need
    // goes through the context's interner. `None` only for a `ctx`-less call
    // (no interner reachable) — those degrade to an empty name rather than
    // panicking on an unresolved `Atom`.
    let default_interner = varn_core::AtomInterner::new();
    let interner = ctx.and_then(|c| c.interner()).unwrap_or(&default_interner);
    // An alias body imported from another module carries that module's
    // `Atom`s; `interner` here is this context's snapshot, which may not have
    // them yet (a nested bind grew the live table after this snapshot). Fall
    // back to the resolver's live table by TEXT rather than degrading the name
    // to "" — an empty name silently resolves nothing downstream.
    let resolve_name = |a: varn_core::Atom| -> Rc<str> {
        if let Some(s) = interner.try_resolve(a) {
            return Rc::from(s);
        }
        ctx.and_then(|c| c.resolver())
            .and_then(|r| r.interner_snapshot().try_resolve(a).map(|s| s.to_owned()))
            .map(Rc::from)
            .unwrap_or_default()
    };
    match &node.kind {
        TypeKind::Intrinsic(TypeTag::Int) => Type::Int,
        TypeKind::Intrinsic(TypeTag::Float) => Type::Float,
        TypeKind::Intrinsic(TypeTag::Decimal) => Type::Decimal,
        TypeKind::Intrinsic(TypeTag::BigInt) => Type::BigInt,
        TypeKind::Intrinsic(TypeTag::Str) => Type::Str,
        TypeKind::Intrinsic(TypeTag::Char) => Type::Char,
        TypeKind::Intrinsic(TypeTag::Bool) => Type::Bool,
        TypeKind::Intrinsic(TypeTag::Symbol) => Type::Symbol,
        TypeKind::Intrinsic(TypeTag::Void) => Type::Void,
        TypeKind::Intrinsic(TypeTag::Null) => Type::Null,
        TypeKind::Intrinsic(TypeTag::Never) => Type::Never,
        TypeKind::Intrinsic(TypeTag::Dynamic) => Type::Dynamic,
        TypeKind::Intrinsic(TypeTag::Bytes) => Type::intrinsic(TypeTag::Bytes, table),
        TypeKind::This => Type::This,

        TypeKind::Array(inner) => {
            let inner_ty = resolve_type_node(inner, ctx, table);
            Type::array(inner_ty, table)
        }
        TypeKind::Union(members) => {
            let resolved: Vec<Type> = members.iter().map(|m| resolve_type_node(m, ctx, table)).collect();
            Type::union(resolved, table)
        }
        TypeKind::Generic(name, args, _origin) => {
            let name_str = resolve_name(*name);
            let resolved_args: Vec<Type> =
                args.iter().map(|m| resolve_type_node(m, ctx, table)).collect();

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

            // `Array<T>` is the same type as `T[]`; the parser just produces a
            // different node for each spelling. Normalize to the one form the
            // rest of the checker tests for, so both spellings behave — and
            // COMPILE — identically. Without this, every `TypeKind::Array`
            // check silently misses `Array<T>`, and the one that matters most
            // is `record_array_index`: an `Array<int>`-annotated value fell
            // back to the generic `GetIndex`/`SetIndex` opcodes, losing the
            // inline array fast path in both JIT tiers (and bailing the whole
            // function out of CLIF), while the very same code written `int[]`
            // — or with no annotation at all — got it.
            //
            // Placed after the user/stdlib alias lookups so an explicitly
            // declared `Array<T>` alias still wins.
            if name_str.as_ref() == varn_core::IntrinsicType::Array.as_str() {
                if let [el] = resolved_args.as_slice() {
                    return Type::array(*el, table);
                }
            }

            // Origin must point at the DECLARING module, not the file that
            // wrote the annotation: member lookup resolves the class through
            // it (`check_origin_module`). An imported symbol carries its
            // declaring origin; fall back to the current file only for
            // locally-declared (or unresolvable) names.
            let resolver = ctx.and_then(|c| c.resolver());
            let origin = ctx
                .and_then(|c| c.resolve_symbol(&name_str))
                .and_then(|t| match table.get(t.0) {
                    TypeKind::Named(_, o) | TypeKind::Generic(_, _, o) => o,
                    _ => None,
                })
                .or_else(|| {
                    ctx.and_then(|c| c.source_file())
                        .and_then(|s| resolver.map(|r| r.intern(s)))
                });
            // `*name` is already an `Atom` from the SAME shared interner as
            // `ctx` (the AST and the checker draw from one per-compilation
            // `AtomInterner`) — no need to round-trip through `name_str` and
            // re-intern.
            Type::generic_atom(*name, resolved_args, origin, table)
        }
        TypeKind::Named(name, __origin) => {
            let name_str = resolve_name(*name);
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
                return resolved;
            }

            prim
        }
        TypeKind::Fn((params, ret)) => {
            let resolved_params = params
                .iter()
                .map(|p| {
                    let ty = p
                        .constraint
                        .as_ref()
                        .map(|m| resolve_type_node(m, ctx, table))
                        .unwrap_or(Type::Dynamic);
                    crate::types::FunctionParam {
                        name: Some(resolve_name(p.name)),
                        ty: ty.0,
                        optional: false,
                        is_rest: false,
                    }
                })
                .collect();
            let ret_ty = resolve_type_node(ret, ctx, table);
            Type::fn_(
                FunctionType {
                    params: resolved_params,
                    return_type: ret_ty.0,
                    is_arrow: false,
                    type_params: vec![],
                },
                table,
            )
        }
        TypeKind::Object(members) => {
            let resolved_members: Vec<ObjectTypeMember> = members
                .iter()
                .map(|m| match m {
                    varn_core::ast::InterfaceMember::Property {
                        key,
                        type_ann,
                        optional,
                        readonly,
                        ..
                    } => {
                        let ty = resolve_type_node(type_ann, ctx, table);
                        ObjectTypeMember::Property {
                            name: resolve_name(*key),
                            ty: ty.0,
                            optional: *optional,
                            readonly: *readonly,
                        }
                    }
                    varn_core::ast::InterfaceMember::Method {
                        key,
                        params,
                        return_type,
                        optional,
                        is_async,
                        ..
                    } => {
                        let resolved_params = params
                            .iter()
                            .map(|p| {
                                let mut ty = p
                                    .type_ann
                                    .as_ref()
                                    .or(match &p.pattern {
                                        varn_core::ast::Pattern::Identifier {
                                            type_ann, ..
                                        } => type_ann.as_ref(),
                                        _ => None,
                                    })
                                    .map(|ann| resolve_type_node(ann, ctx, table))
                                    .unwrap_or(Type::Dynamic);
                                if p.is_rest && !matches!(table.get(ty.0), TypeKind::Array(_)) {
                                    ty = Type::array(ty, table);
                                }
                                crate::types::FunctionParam {
                                    name: Some(Rc::from(crate::binder::pattern_lead_name(
                                        &p.pattern,
                                        interner,
                                    ))),
                                    ty: ty.0,
                                    optional: p.is_optional || p.default.is_some(),
                                    is_rest: p.is_rest,
                                }
                            })
                            .collect::<Vec<_>>();

                        let ret_resolved = return_type
                            .as_ref()
                            .map(|m| resolve_type_node(m, ctx, table))
                            .unwrap_or(Type::Dynamic);
                        let ret = crate::types::async_fn_return(
                            ret_resolved,
                            *is_async,
                            table,
                            interner,
                            ctx.and_then(|c| c.resolver()),
                        );
                        ObjectTypeMember::Method {
                            name: resolve_name(*key),
                            params: resolved_params,
                            return_type: ret.0,
                            optional: *optional,
                            is_arrow: false,
                        }
                    }
                    varn_core::ast::InterfaceMember::Index {
                        param, return_type, ..
                    } => {
                        let key_ty = param
                            .type_ann
                            .as_ref()
                            .or(match &param.pattern {
                                varn_core::ast::Pattern::Identifier { type_ann, .. } => {
                                    type_ann.as_ref()
                                }
                                _ => None,
                            })
                            .map(|ann| resolve_type_node(ann, ctx, table))
                            .unwrap_or(Type::Str);
                        let value_ty = resolve_type_node(return_type, ctx, table);
                        ObjectTypeMember::Index {
                            param_name: Rc::from(crate::binder::pattern_lead_name(
                                &param.pattern,
                                interner,
                            )),
                            key_ty: key_ty.0,
                            value_ty: value_ty.0,
                        }
                    }
                    varn_core::ast::InterfaceMember::Callable {
                        params,
                        return_type,
                        ..
                    } => {
                        let resolved_params = params
                            .iter()
                            .map(|p| {
                                let mut ty = p
                                    .type_ann
                                    .as_ref()
                                    .or(match &p.pattern {
                                        varn_core::ast::Pattern::Identifier {
                                            type_ann, ..
                                        } => type_ann.as_ref(),
                                        _ => None,
                                    })
                                    .map(|ann| resolve_type_node(ann, ctx, table))
                                    .unwrap_or(Type::Dynamic);
                                if p.is_rest && !matches!(table.get(ty.0), TypeKind::Array(_)) {
                                    ty = Type::array(ty, table);
                                }
                                crate::types::FunctionParam {
                                    name: Some(Rc::from(crate::binder::pattern_lead_name(
                                        &p.pattern,
                                        interner,
                                    ))),
                                    ty: ty.0,
                                    optional: p.is_optional || p.default.is_some(),
                                    is_rest: p.is_rest,
                                }
                            })
                            .collect::<Vec<_>>();
                        let ret_ty = resolve_type_node(return_type, ctx, table);
                        ObjectTypeMember::Callable {
                            params: resolved_params,
                            return_type: ret_ty.0,
                            is_arrow: false,
                        }
                    }
                })
                .collect();
            Type::object(resolved_members, table)
        }
        TypeKind::TemplateLiteral(parts) => resolve_template_literal_type(parts, ctx, table),

        TypeKind::Typeof(expr) => match ctx.and_then(|c| c.ast_arena()) {
            Some(arena) => crate::binder::infer_expr_type(*expr, arena, ctx, table),
            None => Type::Dynamic,
        },

        TypeKind::Intersection(members) => {
            let resolved: Vec<Type> =
                members.iter().map(|m| resolve_type_node(m, ctx, table)).collect();

            let primitives: Vec<Type> = resolved
                .iter()
                .copied()
                .filter(|m| is_primitive_type(m, table))
                .collect();
            if primitives.len() > 1 {
                let first = primitives[0];
                let first_kind = table.get(first.0).clone();
                let incompatible = primitives
                    .iter()
                    .any(|m| std::mem::discriminant(&table.get(m.0)) != std::mem::discriminant(&first_kind));
                if incompatible {
                    return Type::Never;
                }
            }

            let parts_opt: Option<Vec<Vec<ObjectTypeMember>>> = resolved
                .iter()
                .map(|m| match table.get(m.0).clone() {
                    TypeKind::Object(mid) => Some(table.get_object_members(mid).to_vec()),
                    TypeKind::Named(name, origin) => {
                        let ctx = ctx?;
                        let interner = ctx.interner()?;
                        let name_str = interner.resolve(name);
                        let origin_str = origin.map(|o| interner.resolve(o));
                        let members = ctx
                            .get_class_members(name_str, origin_str)
                            .or_else(|| ctx.get_interface_members(name_str, origin_str))?;
                        Some(
                            members
                                .iter()
                                .map(|cm| {
                                    use crate::types::ClassMemberKind;
                                    match cm.kind {
                                        ClassMemberKind::Method => {
                                            if let TypeKind::Fn(fid) = table.get(cm.ty.0) {
                                                let ft = table.get_function(fid).clone();
                                                ObjectTypeMember::Method {
                                                    name: cm.name.clone(),
                                                    params: ft.params.clone(),
                                                    return_type: ft.return_type,
                                                    optional: cm.is_optional,
                                                    is_arrow: ft.is_arrow,
                                                }
                                            } else {
                                                ObjectTypeMember::Property {
                                                    name: cm.name.clone(),
                                                    ty: cm.ty.0,
                                                    optional: cm.is_optional,
                                                    readonly: cm.is_readonly,
                                                }
                                            }
                                        }
                                        _ => ObjectTypeMember::Property {
                                            name: cm.name.clone(),
                                            ty: cm.ty.0,
                                            optional: cm.is_optional,
                                            readonly: cm.is_readonly,
                                        },
                                    }
                                })
                                .collect(),
                        )
                    }
                    _ => None,
                })
                .collect();

            if let Some(parts) = parts_opt {
                return Type::object(parts.into_iter().flatten().collect(), table);
            }
            let ids: Vec<crate::types::CheckerTyId> = resolved.iter().map(|t| t.0).collect();
            let list = table.intern_list(&ids);
            Type(table.intern(TypeKind::Intersection(list)), false)
        }

        TypeKind::KeyOf(inner) => {
            let resolved = resolve_type_node(inner, ctx, table);
            resolve_keyof(resolved, ctx, table)
        }

        TypeKind::IndexedAccess { object, index } => {
            let obj = resolve_type_node(object, ctx, table);
            let idx = resolve_type_node(index, ctx, table);
            resolve_indexed_access(obj, idx, ctx, table)
        }

        TypeKind::Mapped {
            key_var,
            source,
            value,
            optional,
            readonly,
        } => {
            let resolved_source = resolve_type_node(source, ctx, table);

            let source_obj = if let TypeKind::KeyOf(inner) = &source.kind {
                Some(resolve_type_node(inner, ctx, table))
            } else {
                None
            };
            resolve_mapped(
                interner.try_resolve(*key_var).unwrap_or(""),
                resolved_source,
                value,
                *optional,
                *readonly,
                source_obj,
                ctx,
                table,
            )
        }

        TypeKind::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            let check_ty = resolve_type_node(check, ctx, table);
            resolve_conditional(check, &check_ty, extends, true_type, false_type, ctx, table)
        }

        TypeKind::Infer(_) => Type::Dynamic.tainted(),

        TypeKind::TypePredicate {
            parameter_name,
            target_type,
        } => {
            let target = resolve_type_node(target_type, ctx, table);
            let interned = table.intern(TypeKind::TypePredicate {
                parameter_name: *parameter_name,
                target_type: target.0,
            });
            Type(interned, false)
        }

        _ => Type::Dynamic.tainted(),
    }
}
