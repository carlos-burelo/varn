mod index;
mod traverse;
mod types;

use crate::binder::{BindResult, BindView, PendingEnrich};
use crate::types::Type;
use index::build_enrich_context;
use std::sync::Arc;
use traverse::{collect_inferred_return_types_raw, enrich_stmts_for_vars};
use varn_core::ast::AstArena;

/// `Type` is a hash-consed id now, so a function's return type cannot be
/// mutated in place the way the old owned-tree `Type` allowed — this builds
/// the replacement `Type` (same params/is_arrow/type_params, new return
/// type) instead. Non-`Fn` types pass through unchanged: enrichment only
/// ever reaches this once it already knows the symbol is a function.
fn with_new_return_type(
    old: Type,
    new_ret: Type,
    table: &mut crate::types::CheckerTyTable,
) -> Type {
    if let varn_core::TypeKind::Fn(fid) = table.get(old.0) {
        let mut ft = table.get_function(fid).clone();
        ft.return_type = new_ret.0;
        crate::types::Type::fn_(ft, table)
    } else {
        old
    }
}

/// `resolver` is threaded in rather than reached for ambiently: enrichment
/// infers call return types, which means resolving imported signatures.
pub fn enrich_call_returns(
    bind: &mut BindResult,
    ast_arena: &AstArena,
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
                let mut table = std::mem::take(&mut *std::sync::Arc::make_mut(&mut bind.ty_table));
                let ty = crate::checker_call_types::infer_call_type(
                    &ctx.fn_map,
                    &ctx.fn_type_params,
                    &ctx.class_methods,
                    &sym_map,
                    *init,
                    ast_arena,
                    Some(&BindView::new(bind, resolver)),
                    None,
                    &bind.interner,
                    &mut table,
                );
                bind.ty_table = std::sync::Arc::new(table);
                if let Some(t) = ty {
                    let name_atom = bind.arena.get(*sym_id).name;
                    let name: Arc<str> = Arc::from(bind.interner.resolve(name_atom));
                    bind.arena.get_mut(*sym_id).ty = Some(t.clone());
                    sym_map.insert(name, t);
                }
            }

            PendingEnrich::Fn {
                sym_id,
                body,
                is_async,
            } => {
                let stmt: varn_core::ast::StmtId = *body;
                let mut table = std::mem::take(&mut *std::sync::Arc::make_mut(&mut bind.ty_table));
                let inferred = collect_inferred_return_types_raw(
                    &ctx,
                    &sym_map,
                    stmt,
                    ast_arena,
                    &BindView::new(bind, resolver),
                    None,
                    &mut table,
                );
                let inferred = never_when_diverging(inferred, stmt, ast_arena);
                let returns_value = !inferred.is_empty();
                let ret_ty = types::join_types(inferred, &mut table);
                if returns_value {
                    let final_ret = crate::types::async_fn_return(
                        ret_ty,
                        *is_async,
                        &mut table,
                        &bind.interner,
                        Some(resolver),
                    );
                    bind.ty_table = std::sync::Arc::new(table);
                    if let Some(old_ty) = bind.arena.get(*sym_id).ty {
                        let new_ty = with_new_return_type(
                            old_ty,
                            final_ret,
                            &mut *std::sync::Arc::make_mut(&mut bind.ty_table),
                        );
                        bind.arena.get_mut(*sym_id).ty = Some(new_ty);
                    }
                } else {
                    bind.ty_table = std::sync::Arc::new(table);
                }
                enrich_stmts_for_vars(&ctx, &mut sym_map, stmt, ast_arena, bind, resolver, None);
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
                // `Arc<str>`-keyed (out of this task's scope). Resolved to an
                // owned `String` (not a borrow of `bind.interner`) because
                // several call sites below take `bind: &mut BindResult` as a
                // whole, which a live borrow of `bind.interner` would block.
                let class_name_str = bind.interner.resolve(*class_name).to_string();
                let key_str = bind.interner.resolve(*key).to_string();
                let stmt: varn_core::ast::StmtId = *body;
                let mut table = std::mem::take(&mut *std::sync::Arc::make_mut(&mut bind.ty_table));
                let inferred = collect_inferred_return_types_raw(
                    &ctx,
                    &sym_map,
                    stmt,
                    ast_arena,
                    &BindView::new(bind, resolver),
                    Some(&class_name_str),
                    &mut table,
                );
                let inferred = never_when_diverging(inferred, stmt, ast_arena);
                let returns_value = !inferred.is_empty();
                let ret = types::join_types(inferred, &mut table);
                bind.ty_table = std::sync::Arc::new(table);
                if returns_value {
                    let final_ret = crate::types::async_fn_return(
                        ret,
                        *is_async,
                        &mut *std::sync::Arc::make_mut(&mut bind.ty_table),
                        &bind.interner,
                        Some(resolver),
                    );
                    if let Some(old) = bind
                        .class_methods
                        .get(class_name_str.as_str())
                        .and_then(|m| m.get(key_str.as_str()))
                        .copied()
                    {
                        let new_ty = with_new_return_type(
                            old,
                            final_ret,
                            &mut *std::sync::Arc::make_mut(&mut bind.ty_table),
                        );
                        if let Some(ft_map) = bind.class_methods.get_mut(class_name_str.as_str()) {
                            ft_map.insert(Arc::from(key_str.as_str()), new_ty);
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
                            let new_ty = with_new_return_type(
                                m.ty,
                                final_ret,
                                &mut *std::sync::Arc::make_mut(&mut bind.ty_table),
                            );
                            m.ty = new_ty;
                            if let Some(symbol_id) = m.symbol_id {
                                if let Some(old) = bind.arena.get(symbol_id).ty {
                                    let new_ty = with_new_return_type(
                                        old,
                                        final_ret,
                                        &mut *std::sync::Arc::make_mut(&mut bind.ty_table),
                                    );
                                    bind.arena.get_mut(symbol_id).ty = Some(new_ty);
                                }
                            }
                        }
                    }
                }
                enrich_stmts_for_vars(
                    &ctx,
                    &mut sym_map,
                    stmt,
                    ast_arena,
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
                let stmt: varn_core::ast::StmtId = *body;
                let mut table = std::mem::take(&mut *std::sync::Arc::make_mut(&mut bind.ty_table));
                let inferred = collect_inferred_return_types_raw(
                    &ctx,
                    &sym_map,
                    stmt,
                    ast_arena,
                    &BindView::new(bind, resolver),
                    Some(&class_name_str),
                    &mut table,
                );
                let ret = types::join_types(inferred, &mut table);
                bind.ty_table = std::sync::Arc::new(table);
                if !ret.is_dynamic() {
                    if let Some(ft_map) = bind.type_members.getters.get_mut(class_name_str.as_str())
                    {
                        if let Some(ty) = ft_map.get_mut(key_str.as_str()) {
                            *ty = ret;
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
                            m.ty = ret;
                            if let Some(symbol_id) = m.symbol_id {
                                bind.arena.get_mut(symbol_id).ty = Some(ret);
                            }
                        }
                    }
                }
                enrich_stmts_for_vars(
                    &ctx,
                    &mut sym_map,
                    stmt,
                    ast_arena,
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
                let stmt: varn_core::ast::StmtId = *body;
                enrich_stmts_for_vars(
                    &ctx,
                    &mut sym_map,
                    stmt,
                    ast_arena,
                    bind,
                    resolver,
                    Some(&class_name_str),
                );
            }
        }
    }
}

/// A body with no `return` that cannot complete normally (it always throws)
/// returns `never`, not `void`.
fn never_when_diverging(
    inferred: Vec<Type>,
    body: varn_core::ast::StmtId,
    ast_arena: &varn_core::ast::AstArena,
) -> Vec<Type> {
    if inferred.is_empty() && !crate::checker::completion::can_complete_normally(body, ast_arena) {
        vec![Type::Never]
    } else {
        inferred
    }
}
