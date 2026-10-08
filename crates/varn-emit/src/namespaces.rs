use super::body::FnEmitter;
use super::decorators::apply_one_decorator;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::{AstArena, Decl, ExportDecl, NamespaceDecl, Pattern};
use varn_core::{Atom, AtomInterner};
use varn_tir::{
    BackendTy, DynReason, FnId, Resolution, Span, TirExpr, TirExprKind, TirObjectEntry, TirStmt,
};

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
    fn_index: &FxHashMap<Atom, (u32, u32)>,
    global_slots: &FxHashMap<Arc<str>, u32>,
    interner: &AtomInterner,
    shadowed: &rustc_hash::FxHashSet<u32>,
    top: &mut FnEmitter,
    top_body: &mut Vec<TirStmt>,
) {
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
                fn_index,
                global_slots,
                interner,
                shadowed,
                top,
                top_body,
            );
        }
    }

    let Some(&slot) = global_slots.get(interner.resolve(ns.id)) else {
        return;
    };
    let mut entries: Vec<TirObjectEntry> = Vec::new();
    for m in &ns.body {
        let Decl::Export(_) = m else { continue };
        let inner = match m {
            Decl::Export(ExportDecl::Decl { declaration, .. }) => declaration.as_ref(),
            Decl::Variable(_)
            | Decl::Function(_)
            | Decl::Class(_)
            | Decl::Interface(_)
            | Decl::TypeAlias(_)
            | Decl::Enum(_)
            | Decl::Namespace(_)
            | Decl::Import(_)
            | Decl::Export(_)
            | Decl::Extension(_)
            | Decl::Struct(_)
            | Decl::SumType(_) => continue,
        };
        match inner {
            Decl::Function(f) => {
                if let Some(&(fnid, _)) = fn_index.get(&f.id) {
                    let mut value = TirExpr {
                        kind: TirExprKind::Var,
                        ty: dyno(),
                        res: Resolution::DirectFn(FnId(fnid)),
                        span: Span::EMPTY,
                    };
                    for d in f.decorators.iter().rev().filter(|d| {
                        !varn_core::ast::decorators::is_active_builtin(
                            top.ast_arena,
                            interner,
                            |off| shadowed.contains(&off),
                            d,
                        )
                    }) {
                        let deco = top.lower_expression(d.expression);
                        value = apply_one_decorator(top, value, deco);
                        top_body.extend(top.take_pending());
                    }
                    entries.push(TirObjectEntry::Field {
                        name: Arc::from(interner.resolve(f.id)),
                        value,
                    });
                }
            }
            Decl::Class(_) | Decl::Enum(_) | Decl::Namespace(_) => {
                let mname = match inner {
                    Decl::Class(c) => c.id,
                    Decl::Enum(e) => Some(e.id),
                    Decl::Namespace(n) => Some(n.id),
                    Decl::Variable(_)
                    | Decl::Function(_)
                    | Decl::Interface(_)
                    | Decl::TypeAlias(_)
                    | Decl::Import(_)
                    | Decl::Export(_)
                    | Decl::Extension(_)
                    | Decl::Struct(_)
                    | Decl::SumType(_) => None,
                };
                if let Some(mname) = mname {
                    if let Some(&mslot) = global_slots.get(interner.resolve(mname)) {
                        entries.push(TirObjectEntry::Field {
                            name: Arc::from(interner.resolve(mname)),
                            value: global_ref(mslot),
                        });
                    }
                }
            }
            Decl::Variable(v) => {
                for decl in &v.declarators {
                    if let (Pattern::Identifier { name, .. }, Some(init)) = (&decl.id, decl.init) {
                        let value = top.lower_expression(init);
                        entries.push(TirObjectEntry::Field {
                            name: Arc::from(interner.resolve(*name)),
                            value,
                        });
                    }
                }
            }
            Decl::Interface(_)
            | Decl::TypeAlias(_)
            | Decl::Import(_)
            | Decl::Export(_)
            | Decl::Extension(_)
            | Decl::Struct(_)
            | Decl::SumType(_) => {}
        }
    }

    let obj = TirExpr {
        kind: TirExprKind::ObjectLit { entries },
        ty: dyno(),
        res: Resolution::None,
        span: Span::EMPTY,
    };
    top_body.push(TirStmt::Expr(TirExpr {
        kind: TirExprKind::Assign {
            target: Box::new(global_ref(slot)),
            value: Box::new(obj),
        },
        ty: BackendTy::Void,
        res: Resolution::None,
        span: Span::EMPTY,
    }));
}

pub(super) fn collect_decl_names(
    decl: &Decl,
    ast_arena: &AstArena,
    out: &mut FxHashSet<Arc<str>>,
    interner: &AtomInterner,
) {
    use varn_core::ast::{ImportSpecifier, Pattern as P};
    fn pat_names(p: &P, out: &mut FxHashSet<Arc<str>>, interner: &AtomInterner) {
        match p {
            P::Identifier { name, .. } => {
                out.insert(Arc::from(interner.resolve(*name)));
            }
            P::Array { elements, rest, .. } => {
                for e in elements.iter().flatten() {
                    pat_names(&e.pattern, out, interner);
                }
                if let Some(r) = rest {
                    pat_names(r, out, interner);
                }
            }
            P::Object {
                properties, rest, ..
            } => {
                for prop in properties {
                    pat_names(&prop.value, out, interner);
                }
                if let Some(r) = rest {
                    pat_names(r, out, interner);
                }
            }
            P::Assignment { left, .. } => pat_names(left, out, interner),
            P::Rest { argument, .. } => pat_names(argument, out, interner),
        }
    }
    match decl {
        Decl::Function(f) => {
            out.insert(Arc::from(interner.resolve(f.id)));
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
                pat_names(&d.id, out, interner);
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
            out.insert(Arc::from(interner.resolve(ns.id)));
            for m in &ns.body {
                collect_decl_names(m, ast_arena, out, interner);
            }
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
            collect_decl_names(declaration, ast_arena, out, interner)
        }
        Decl::Interface(_)
        | Decl::TypeAlias(_)
        | Decl::Export(_)
        | Decl::Extension(_)
        | Decl::Struct(_)
        | Decl::SumType(_) => {}
    }
}
