use super::body::FnEmitter;
use super::decorators::apply_one_decorator;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::{AstArena, Decl, ExportDecl, NamespaceDecl, Pattern};
use varn_core::AtomInterner;
use varn_tir::{
    BackendTy, DynReason, FnId, Resolution, Span, TirExpr, TirExprKind, TirStmt,
};

pub(crate) fn scoped_name(outer: Option<&str>, name: &str) -> Arc<str> {
    match outer {
        Some(outer) => Arc::from(format!("{outer}::{name}")),
        None => Arc::from(name),
    }
}

pub(super) fn ns_nested_types(ns: &NamespaceDecl) -> Vec<&Decl> {
    let mut out = Vec::new();
    for m in &ns.body {
        let inner = match m {
            Decl::Export(ExportDecl::Decl { declaration, .. }) => declaration.as_ref(),
            other @ Decl::Variable(_)
            | other @ Decl::Function(_)
            | other @ Decl::Class(_)
            | other @ Decl::Interface(_)
            | other @ Decl::TypeAlias(_)
            | other @ Decl::Enum(_)
            | other @ Decl::Namespace(_)
            | other @ Decl::Import(_)
            | other @ Decl::Export(_)
            | other @ Decl::Extension(_)
            | other @ Decl::Struct(_)
            | other @ Decl::SumType(_) => other,
        };
        match inner {
            Decl::Class(_) | Decl::Enum(_) => out.push(inner),
            Decl::Namespace(inner_ns) => out.extend(ns_nested_types(inner_ns)),
            Decl::Variable(_)
            | Decl::Function(_)
            | Decl::Interface(_)
            | Decl::TypeAlias(_)
            | Decl::Import(_)
            | Decl::Export(_)
            | Decl::Extension(_)
            | Decl::Struct(_)
            | Decl::SumType(_) => {}
        }
    }
    out
}

pub(super) fn emit_namespace_object(
    ns: &NamespaceDecl,
    outer: Option<&str>,
    scoped_fns: &FxHashMap<Arc<str>, (u32, u32)>,
    global_slots: &FxHashMap<Arc<str>, u32>,
    interner: &AtomInterner,
    shadowed: &rustc_hash::FxHashSet<u32>,
    top: &mut FnEmitter,
    top_body: &mut Vec<TirStmt>,
) {
    let path = scoped_name(outer, interner.resolve(ns.id));
    let dyno = || BackendTy::Dynamic(DynReason::NotYetSupported);
    let global_ref = |slot: u32| TirExpr {
        kind: TirExprKind::Var,
        ty: dyno(),
        res: Resolution::GlobalSlot(slot),
        span: Span::EMPTY,
    };

    for m in &ns.body {
        let inner = match m {
            Decl::Export(ExportDecl::Decl { declaration, .. }) => declaration.as_ref(),
            other @ Decl::Variable(_)
            | other @ Decl::Function(_)
            | other @ Decl::Class(_)
            | other @ Decl::Interface(_)
            | other @ Decl::TypeAlias(_)
            | other @ Decl::Enum(_)
            | other @ Decl::Namespace(_)
            | other @ Decl::Import(_)
            | other @ Decl::Export(_)
            | other @ Decl::Extension(_)
            | other @ Decl::Struct(_)
            | other @ Decl::SumType(_) => other,
        };
        if let Decl::Namespace(inner_ns) = inner {
            emit_namespace_object(
                inner_ns,
                Some(&path),
                scoped_fns,
                global_slots,
                interner,
                shadowed,
                top,
                top_body,
            );
        }
    }

    if !global_slots.contains_key(&path) {
        return;
    }
    let outer_scope = top.namespace.replace(path.clone());
    let mut pending: Vec<Arc<str>> = Vec::new();
    for m in &ns.body {
        let inner = if let Decl::Export(ExportDecl::Decl { declaration, .. }) = m {
            declaration.as_ref()
        } else {
            m
        };
        if let Decl::Variable(v) = inner {
            for decl in &v.declarators {
                if let Pattern::Identifier { name, .. } = &decl.id {
                    pending.push(scoped_name(Some(&path), interner.resolve(*name)));
                }
            }
        }
    }
    let outer_pending = std::mem::replace(
        &mut top.not_yet_bound,
        pending.iter().cloned().collect(),
    );
    for m in &ns.body {
        let inner = if let Decl::Export(ExportDecl::Decl { declaration, .. }) = m {
            declaration.as_ref()
        } else {
            m
        };
        let Decl::Variable(v) = inner else { continue };
        for decl in &v.declarators {
            let (Pattern::Identifier { name, .. }, Some(init)) = (&decl.id, decl.init) else {
                continue;
            };
            let Some(&held) = global_slots.get(&scoped_name(Some(&path), interner.resolve(*name)))
            else {
                continue;
            };
            let value = top.lower_expression(init);
            top_body.extend(top.take_pending());
            top_body.push(TirStmt::Expr(TirExpr {
                kind: TirExprKind::Assign {
                    target: Box::new(global_ref(held)),
                    value: Box::new(value),
                },
                ty: BackendTy::Void,
                res: Resolution::None,
                span: Span::EMPTY,
            }));
            top.not_yet_bound
                .remove(&scoped_name(Some(&path), interner.resolve(*name)));
        }
    }
    for m in &ns.body {
        let inner = if let Decl::Export(ExportDecl::Decl { declaration, .. }) = m {
            declaration.as_ref()
        } else {
            m
        };
        let Decl::Function(f) = inner else { continue };
        let key = scoped_name(Some(&path), interner.resolve(f.id));
        let (Some(&(fnid, _)), Some(&held)) = (scoped_fns.get(&key), global_slots.get(&key))
        else {
            continue;
        };
        let wrappers: Vec<_> = f
            .decorators
            .iter()
            .rev()
            .filter(|d| {
                !varn_core::ast::decorators::is_active_builtin(
                    top.ast_arena,
                    interner,
                    |off| shadowed.contains(&off),
                    d,
                )
            })
            .collect();
        if wrappers.is_empty() {
            continue;
        }
        let mut value = TirExpr {
            kind: TirExprKind::Var,
            ty: dyno(),
            res: Resolution::DirectFn(FnId(fnid)),
            span: Span::EMPTY,
        };
        for d in wrappers {
            let deco = top.lower_expression(d.expression);
            value = apply_one_decorator(top, value, deco);
            top_body.extend(top.take_pending());
        }
        top_body.push(TirStmt::Expr(TirExpr {
            kind: TirExprKind::Assign {
                target: Box::new(global_ref(held)),
                value: Box::new(value),
            },
            ty: BackendTy::Void,
            res: Resolution::None,
            span: Span::EMPTY,
        }));
    }

    top.namespace = outer_scope;
    top.not_yet_bound = outer_pending;
}

pub(super) fn collect_decl_names(
    decl: &Decl,
    ast_arena: &AstArena,
    out: &mut FxHashSet<Arc<str>>,
    interner: &AtomInterner,
) {
    collect_scoped_names(decl, None, ast_arena, out, interner)
}

fn collect_scoped_names(
    decl: &Decl,
    scope: Option<&str>,
    ast_arena: &AstArena,
    out: &mut FxHashSet<Arc<str>>,
    interner: &AtomInterner,
) {
    use varn_core::ast::{ImportSpecifier, Pattern as P};
    fn pat_names(
        p: &P,
        scope: Option<&str>,
        out: &mut FxHashSet<Arc<str>>,
        interner: &AtomInterner,
    ) {
        match p {
            P::Identifier { name, .. } => {
                out.insert(scoped_name(scope, interner.resolve(*name)));
            }
            P::Array { elements, rest, .. } => {
                for e in elements.iter().flatten() {
                    pat_names(&e.pattern, scope, out, interner);
                }
                if let Some(r) = rest {
                    pat_names(r, scope, out, interner);
                }
            }
            P::Object {
                properties, rest, ..
            } => {
                for prop in properties {
                    pat_names(&prop.value, scope, out, interner);
                }
                if let Some(r) = rest {
                    pat_names(r, scope, out, interner);
                }
            }
            P::Assignment { left, .. } => pat_names(left, scope, out, interner),
            P::Rest { argument, .. } => pat_names(argument, scope, out, interner),
        }
    }
    match decl {
        Decl::Function(f) => {
            out.insert(scoped_name(scope, interner.resolve(f.id)));
        }
        Decl::Class(c) => {
            if let Some(id) = &c.id {
                out.insert(Arc::from(interner.resolve(*id)));
            }
        }
        Decl::Enum(e) => {
            out.insert(Arc::from(interner.resolve(e.id)));
        }
        Decl::Variable(v) => {
            for d in &v.declarators {
                pat_names(&d.id, scope, out, interner);
                if d.init.is_some_and(|e| {
                    matches!(
                        ast_arena.expr(e).kind,
                        varn_core::ast::ExprKind::ClassExpr { .. }
                    )
                }) {
                    out.insert(Arc::from("<anon>"));
                }
            }
        }
        Decl::Namespace(ns) => {
            let path = scoped_name(scope, interner.resolve(ns.id));
            for m in &ns.body {
                collect_scoped_names(m, Some(&path), ast_arena, out, interner);
            }
            out.insert(path);
        }
        Decl::Import(i) => {
            for spec in &i.specifiers {
                let (ImportSpecifier::Default { local, .. }
                | ImportSpecifier::Named { local, .. }
                | ImportSpecifier::Namespace { local, .. }) = spec;
                out.insert(Arc::from(interner.resolve(*local)));
            }
        }
        Decl::Export(ExportDecl::Decl { declaration, .. }) => {
            collect_scoped_names(declaration, scope, ast_arena, out, interner)
        }
        Decl::Interface(_)
        | Decl::TypeAlias(_)
        | Decl::Export(_)
        | Decl::Extension(_)
        | Decl::Struct(_)
        | Decl::SumType(_) => {}
    }
}
