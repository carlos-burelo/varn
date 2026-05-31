mod index;
mod traverse;
mod types;

use crate::binder::{BindResult, PendingEnrich};
use index::build_enrich_context;
use traverse::{collect_inferred_return_types_raw, enrich_stmts_for_vars};
use varn_core::ast::{Expr, Stmt};

pub fn enrich_call_returns(bind: &mut BindResult) {
    if bind.pending_enrich.is_empty() {
        return;
    }

    let (ctx, mut sym_map) = build_enrich_context(bind);
    let pending = std::mem::take(&mut bind.pending_enrich);

    for entry in &pending {
        match entry {
            PendingEnrich::Var { sym_id, init } => {
                if bind.arena.get(*sym_id).ty.as_ref().map_or(false, |t| !t.is_dynamic()) {
                    continue;
                }
                let expr: &Expr = unsafe { &**init };
                let ty = crate::checker_call_types::infer_call_type(
                    &ctx.fn_map,
                    &ctx.fn_type_params,
                    &ctx.class_methods,
                    &sym_map,
                    expr,
                    Some(bind),
                    None,
                );
                if let Some(t) = ty {
                    let name = bind.arena.get(*sym_id).name.clone();
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
                let inferred = collect_inferred_return_types_raw(&ctx, &sym_map, stmt, bind, None);
                let ret_ty = types::join_types(inferred);
                if !ret_ty.is_dynamic() {
                    let final_ret = if *is_async {
                        crate::types::Type::generic(
                            varn_core::IntrinsicType::Task.as_str().to_owned(),
                            vec![ret_ty],
                        )
                    } else {
                        ret_ty
                    };
                    let sym = bind.arena.get_mut(*sym_id);
                    if let Some(crate::types::Type(varn_core::TypeKind::Fn(ft), _)) = &mut sym.ty {
                        ft.return_type = Box::new(final_ret);
                    }
                }
                enrich_stmts_for_vars(&ctx, &mut sym_map, stmt, bind, None);
            }

            PendingEnrich::Method {
                class_name,
                key,
                body,
                is_async,
            } => {
                let stmt: &Stmt = unsafe { &**body };
                let inferred = collect_inferred_return_types_raw(
                    &ctx,
                    &sym_map,
                    stmt,
                    bind,
                    Some(class_name.as_ref()),
                );
                let ret = types::join_types(inferred);
                if !ret.is_dynamic() {
                    let final_ret = if *is_async {
                        crate::types::Type::generic(
                            varn_core::IntrinsicType::Task.as_str().to_owned(),
                            vec![ret],
                        )
                    } else {
                        ret
                    };
                    if let Some(ft_map) = bind.class_methods.get_mut(class_name) {
                        if let Some(crate::types::Type(varn_core::TypeKind::Fn(ft), _)) =
                            ft_map.get_mut(key)
                        {
                            ft.return_type = Box::new(final_ret.clone());
                        }
                    }
                    if let Some(class_info) = bind.type_members.classes.get_mut(class_name) {
                        if let Some(m) = class_info
                            .members
                            .iter_mut()
                            .find(|m| m.name.as_ref() == key.as_ref())
                        {
                            if let crate::types::Type(varn_core::TypeKind::Fn(ft), _) = &mut m.ty {
                                ft.return_type = Box::new(final_ret.clone());
                            }
                        }
                    }
                }
                enrich_stmts_for_vars(&ctx, &mut sym_map, stmt, bind, Some(class_name.as_ref()));
            }

            PendingEnrich::Getter {
                class_name,
                key,
                body,
            } => {
                let stmt: &Stmt = unsafe { &**body };
                let inferred = collect_inferred_return_types_raw(
                    &ctx,
                    &sym_map,
                    stmt,
                    bind,
                    Some(class_name.as_ref()),
                );
                let ret = types::join_types(inferred);
                if !ret.is_dynamic() {
                    if let Some(ft_map) = bind.type_members.getters.get_mut(class_name) {
                        if let Some(ty) = ft_map.get_mut(key) {
                            *ty = ret.clone();
                        }
                    }
                    if let Some(class_info) = bind.type_members.classes.get_mut(class_name) {
                        if let Some(m) = class_info
                            .members
                            .iter_mut()
                            .find(|m| m.name.as_ref() == key.as_ref())
                        {
                            m.ty = ret.clone();
                        }
                    }
                }
                enrich_stmts_for_vars(&ctx, &mut sym_map, stmt, bind, Some(class_name.as_ref()));
            }

            PendingEnrich::Setter {
                class_name,
                key: _,
                body,
            } => {
                let stmt: &Stmt = unsafe { &**body };
                enrich_stmts_for_vars(&ctx, &mut sym_map, stmt, bind, Some(class_name.as_ref()));
            }
        }
    }
}
