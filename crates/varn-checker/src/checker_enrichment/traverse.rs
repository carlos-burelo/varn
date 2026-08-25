use crate::binder::{BindResult, BindView};
use crate::checker_call_types::infer_call_type;
use crate::checker_enrichment::index::EnrichContext;
use crate::types::Type;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::ast::{Decl, Stmt, StmtKind};

pub(super) fn collect_inferred_return_types_raw(
    ctx: &EnrichContext,
    sym_map: &FxHashMap<Rc<str>, Type>,
    stmt: &Stmt,
    bind: &BindView,
    current_class: Option<&str>,
) -> Vec<Type> {
    let mut results = Vec::new();
    collect_returns_recursive(ctx, sym_map, stmt, bind, current_class, &mut results);
    results
}

fn collect_returns_recursive(
    ctx: &EnrichContext,
    sym_map: &FxHashMap<Rc<str>, Type>,
    stmt: &Stmt,
    bind: &BindView,
    current_class: Option<&str>,
    results: &mut Vec<Type>,
) {
    match &stmt.kind {
        StmtKind::Return { argument } => {
            if let Some(arg) = argument {
                if let Some(ty) = infer_call_type(
                    &ctx.fn_map,
                    &ctx.fn_type_params,
                    &ctx.class_methods,
                    sym_map,
                    arg,
                    Some(bind),
                    current_class,
                ) {
                    results.push(ty);
                }
            } else {
                results.push(Type::Void);
            }
        }
        StmtKind::Block { stmts } => {
            for s in stmts {
                collect_returns_recursive(ctx, sym_map, s, bind, current_class, results);
            }
        }
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => {
            collect_returns_recursive(ctx, sym_map, consequent, bind, current_class, results);
            if let Some(alt) = alternate {
                collect_returns_recursive(ctx, sym_map, alt, bind, current_class, results);
            }
        }
        StmtKind::While { body, .. } | StmtKind::DoWhile { body, .. } => {
            collect_returns_recursive(ctx, sym_map, body, bind, current_class, results);
        }
        StmtKind::For { body, .. }
        | StmtKind::ForIn { body, .. }
        | StmtKind::ForOf { body, .. } => {
            collect_returns_recursive(ctx, sym_map, body, bind, current_class, results);
        }
        StmtKind::Switch { cases, .. } => {
            for case in cases {
                for s in &case.body {
                    collect_returns_recursive(ctx, sym_map, s, bind, current_class, results);
                }
            }
        }
        StmtKind::Try {
            block,
            catch,
            finally,
        } => {
            collect_returns_recursive(ctx, sym_map, block, bind, current_class, results);
            if let Some(c) = catch {
                collect_returns_recursive(ctx, sym_map, &c.body, bind, current_class, results);
            }
            if let Some(f) = finally {
                collect_returns_recursive(ctx, sym_map, f, bind, current_class, results);
            }
        }
        StmtKind::Labeled { body, .. } => {
            collect_returns_recursive(ctx, sym_map, body, bind, current_class, results);
        }
        _ => {}
    }
}

pub(super) fn enrich_stmts_for_vars(
    ctx: &EnrichContext,
    sym_map: &mut FxHashMap<Rc<str>, Type>,
    stmt: &Stmt,
    bind: &mut BindResult,
    resolver: &dyn crate::module_resolver::ImportResolver,
    current_class: Option<&str>,
) {
    enrich_vars_recursive(ctx, sym_map, stmt, bind, resolver, current_class);
}

fn enrich_vars_recursive(
    ctx: &EnrichContext,
    sym_map: &mut FxHashMap<Rc<str>, Type>,
    stmt: &Stmt,
    bind: &mut BindResult,
    resolver: &dyn crate::module_resolver::ImportResolver,
    current_class: Option<&str>,
) {
    match &stmt.kind {
        StmtKind::Decl(decl) => {
            if let Decl::Variable(v) = decl.as_ref() {
                use crate::binder::pattern_lead_name;
                for d in &v.declarators {
                    let name = pattern_lead_name(&d.id);
                    let scope = bind.scopes.get(bind.global_scope);
                    let sym_id = match scope.lookup(name) {
                        Some(id) => id,
                        None => continue,
                    };
                    if bind.arena.get(sym_id).ty.is_some() {
                        if let Some(existing) = &bind.arena.get(sym_id).ty {
                            sym_map
                                .entry(Rc::from(name))
                                .or_insert_with(|| existing.clone());
                        }
                        continue;
                    }
                    let init = match &d.init {
                        Some(e) => e,
                        None => continue,
                    };
                    let ty = infer_call_type(
                        &ctx.fn_map,
                        &ctx.fn_type_params,
                        &ctx.class_methods,
                        sym_map,
                        init,
                        Some(&BindView::new(bind, resolver)),
                        current_class,
                    );
                    if let Some(t) = ty {
                        bind.arena.get_mut(sym_id).ty = Some(t.clone());
                        sym_map.insert(Rc::from(name), t);
                    }
                }
            }
        }
        StmtKind::Block { stmts } => {
            for s in stmts {
                enrich_vars_recursive(ctx, sym_map, s, bind, resolver, current_class);
            }
        }
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => {
            enrich_vars_recursive(ctx, sym_map, consequent, bind, resolver, current_class);
            if let Some(alt) = alternate {
                enrich_vars_recursive(ctx, sym_map, alt, bind, resolver, current_class);
            }
        }
        StmtKind::While { body, .. } | StmtKind::DoWhile { body, .. } => {
            enrich_vars_recursive(ctx, sym_map, body, bind, resolver, current_class);
        }
        StmtKind::For { body, .. }
        | StmtKind::ForIn { body, .. }
        | StmtKind::ForOf { body, .. } => {
            enrich_vars_recursive(ctx, sym_map, body, bind, resolver, current_class);
        }
        StmtKind::Switch { cases, .. } => {
            for case in cases {
                for s in &case.body {
                    enrich_vars_recursive(ctx, sym_map, s, bind, resolver, current_class);
                }
            }
        }
        StmtKind::Try {
            block,
            catch,
            finally,
        } => {
            enrich_vars_recursive(ctx, sym_map, block, bind, resolver, current_class);
            if let Some(c) = catch {
                enrich_vars_recursive(ctx, sym_map, &c.body, bind, resolver, current_class);
            }
            if let Some(f) = finally {
                enrich_vars_recursive(ctx, sym_map, f, bind, resolver, current_class);
            }
        }
        StmtKind::Labeled { body, .. } => {
            enrich_vars_recursive(ctx, sym_map, body, bind, resolver, current_class);
        }
        _ => {}
    }
}
