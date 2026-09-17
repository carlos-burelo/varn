mod index;
mod traverse;
mod types;

use crate::binder::{BindResult, BindView, PendingEnrich};
use index::build_enrich_context;
use std::rc::Rc;
use traverse::{collect_inferred_return_types_raw, enrich_stmts_for_vars};
use varn_core::ast::{Expr, Stmt};

/// `resolver` is threaded in rather than reached for ambiently: enrichment
/// infers call return types, which means resolving imported signatures.
pub fn enrich_call_returns(
    bind: &mut BindResult,
    resolver: &dyn crate::module_resolver::ImportResolver,
) {
    if bind.pending_enrich.is_empty() {
        return;
    }

    let (ctx, mut sym_map) = build_enrich_context(bind);
    let pending = std::mem::take(&mut bind.pending_enrich);

    for entry in &pending {
        match entry {
            PendingEnrich::Var { sym_id, init } => {
                if bind
                    .arena
                    .get(*sym_id)
                    .ty
                    .as_ref()
                    .is_some_and(|t| !t.is_dynamic())
                {
                    continue;
                }
                let expr: &Expr = unsafe { &**init };
                let ty = crate::checker_call_types::infer_call_type(
                    &ctx.fn_map,
                    &ctx.fn_type_params,
                    &ctx.class_methods,
                    &sym_map,
                    expr,
                    Some(&BindView::new(bind, resolver)),
                    None,
                    &bind.interner,
                );
                if let Some(t) = ty {
                    let name_atom = bind.arena.get(*sym_id).name;
                    let name: Rc<str> = Rc::from(bind.interner.resolve(name_atom));
                    bind.arena.get_mut(*sym_id).ty = Some(t.clone());
                    sym_map.insert(name, t);
                }
            }

            PendingEnrich::Fn {
                sym_id,
                body,
                is_async,
            } => {
                let stmt: &Stmt = unsafe { &**body };
                let inferred = collect_inferred_return_types_raw(
                    &ctx,
                    &sym_map,
                    stmt,
                    &BindView::new(bind, resolver),
                    None,
                );
                let ret_ty = types::join_types(inferred);
                if !ret_ty.is_dynamic() {
                    let final_ret = crate::types::async_fn_return(ret_ty, *is_async);
                    let sym = bind.arena.get_mut(*sym_id);
                    if let Some(crate::types::Type(varn_core::TypeKind::Fn(ft), _)) = &mut sym.ty {
                        *ft.return_type = final_ret;
                    }
                }
                enrich_stmts_for_vars(&ctx, &mut sym_map, stmt, bind, resolver, None);
            }

            PendingEnrich::Method {
                class_name,
                key,
                body,
                is_async,
            } => {
                // `class_name`/`key` forward the AST `Atom`s recorded in
                // `PendingEnrich` (Task 4); every map consulted here
                // (`class_methods`, `type_members.classes`) is still
                // `Rc<str>`-keyed (out of this task's scope). Resolved to an
                // owned `String` (not a borrow of `bind.interner`) because
                // several call sites below take `bind: &mut BindResult` as a
                // whole, which a live borrow of `bind.interner` would block.
                let class_name_str = bind.interner.resolve(*class_name).to_string();
                let key_str = bind.interner.resolve(*key).to_string();
                let stmt: &Stmt = unsafe { &**body };
                let inferred = collect_inferred_return_types_raw(
                    &ctx,
                    &sym_map,
                    stmt,
                    &BindView::new(bind, resolver),
                    Some(&class_name_str),
                );
                let ret = types::join_types(inferred);
                if !ret.is_dynamic() {
                    let final_ret = crate::types::async_fn_return(ret, *is_async);
                    if let Some(ft_map) = bind.class_methods.get_mut(class_name_str.as_str()) {
                        if let Some(crate::types::Type(varn_core::TypeKind::Fn(ft), _)) =
                            ft_map.get_mut(key_str.as_str())
                        {
                            *ft.return_type = final_ret.clone();
                        }
                    }
                    if let Some(class_info) =
                        bind.type_members.classes.get_mut(class_name_str.as_str())
                    {
                        if let Some(m) = class_info
                            .members
                            .iter_mut()
                            .find(|m| m.name.as_ref() == key_str)
                        {
                            if let crate::types::Type(varn_core::TypeKind::Fn(ft), _) = &mut m.ty {
                                *ft.return_type = final_ret.clone();
                            }
                            if let Some(symbol_id) = m.symbol_id {
                                if let Some(crate::types::Type(varn_core::TypeKind::Fn(ft), _)) =
                                    &mut bind.arena.get_mut(symbol_id).ty
                                {
                                    *ft.return_type = final_ret.clone();
                                }
                            }
                        }
                    }
                }
                enrich_stmts_for_vars(
                    &ctx,
                    &mut sym_map,
                    stmt,
                    bind,
                    resolver,
                    Some(&class_name_str),
                );
            }

            PendingEnrich::Getter {
                class_name,
                key,
                body,
            } => {
                let class_name_str = bind.interner.resolve(*class_name).to_string();
                let key_str = bind.interner.resolve(*key).to_string();
                let stmt: &Stmt = unsafe { &**body };
                let inferred = collect_inferred_return_types_raw(
                    &ctx,
                    &sym_map,
                    stmt,
                    &BindView::new(bind, resolver),
                    Some(&class_name_str),
                );
                let ret = types::join_types(inferred);
                if !ret.is_dynamic() {
                    if let Some(ft_map) = bind
                        .type_members
                        .getters
                        .get_mut(class_name_str.as_str())
                    {
                        if let Some(ty) = ft_map.get_mut(key_str.as_str()) {
                            *ty = ret.clone();
                        }
                    }
                    if let Some(class_info) =
                        bind.type_members.classes.get_mut(class_name_str.as_str())
                    {
                        if let Some(m) = class_info
                            .members
                            .iter_mut()
                            .find(|m| m.name.as_ref() == key_str)
                        {
                            m.ty = ret.clone();
                            if let Some(symbol_id) = m.symbol_id {
                                bind.arena.get_mut(symbol_id).ty = Some(ret.clone());
                            }
                        }
                    }
                }
                enrich_stmts_for_vars(
                    &ctx,
                    &mut sym_map,
                    stmt,
                    bind,
                    resolver,
                    Some(&class_name_str),
                );
            }

            PendingEnrich::Setter {
                class_name,
                key: _,
                body,
            } => {
                let class_name_str = bind.interner.resolve(*class_name).to_string();
                let stmt: &Stmt = unsafe { &**body };
                enrich_stmts_for_vars(
                    &ctx,
                    &mut sym_map,
                    stmt,
                    bind,
                    resolver,
                    Some(&class_name_str),
                );
            }
        }
    }
}
