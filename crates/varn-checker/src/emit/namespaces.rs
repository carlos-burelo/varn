use super::body::FnEmitter;
use super::decorators::apply_one_decorator;
use super::functions::{fresh_sig, param_name};
use super::module_ctx::MCtx;
use super::ty::NameResolver;
use crate::checker::TypeEntry;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::{
    AstArena, AstId, Decl, ExportDecl, NamespaceDecl, Pattern, Program, StmtId, StmtKind,
};
use varn_core::{Atom, AtomInterner};
use varn_tir::{
    BackendTy, DynReason, FnId, Resolution, Signature, Span, TirExpr, TirExprKind, TirFunction,
    TirObjectEntry, TirStmt, TyTable,
};

pub(super) fn ns_nested_types(ns: &NamespaceDecl) -> Vec<&Decl> {
    let mut out = Vec::new();
    for m in &ns.body {
        let inner = match m {
            Decl::Export(ExportDecl::Decl { declaration, .. }) => declaration.as_ref(),
            other => other,
        };
        match inner {
            Decl::Class(_) | Decl::Enum(_) => out.push(inner),
            Decl::Namespace(inner_ns) => out.extend(ns_nested_types(inner_ns)),
            _ => {}
        }
    }
    out
}

pub(super) fn emit_namespace_object(
    ns: &NamespaceDecl,
    fn_index: &FxHashMap<Atom, (u32, u32)>,
    global_slots: &FxHashMap<Arc<str>, u32>,
    interner: &AtomInterner,
    top: &mut FnEmitter,
    top_body: &mut Vec<TirStmt>,
) {
    let dyno = || BackendTy::Dynamic(DynReason::Unannotated);
    let global_ref = |slot: u32| TirExpr {
        kind: TirExprKind::Var,
        ty: dyno(),
        res: Resolution::GlobalSlot(slot),
        span: Span::EMPTY,
    };

    for m in &ns.body {
        let inner = match m {
            Decl::Export(ExportDecl::Decl { declaration, .. }) => declaration.as_ref(),
            other => other,
        };
        if let Decl::Namespace(inner_ns) = inner {
            emit_namespace_object(inner_ns, fn_index, global_slots, interner, top, top_body);
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
            _ => continue,
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
                    for d in f.decorators.iter().rev() {
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
                    _ => None,
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
            _ => {}
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
        _ => {}
    }
}

pub(super) fn collect_extension_names(
    program: &Program,
    ast_arena: &AstArena,
    out: &mut FxHashSet<Arc<str>>,
    interner: &AtomInterner,
) {
    use varn_core::ast::ExtensionMember;
    for &stmt in &program.body {
        let StmtKind::Decl(d) = &ast_arena.stmt(stmt).kind else {
            continue;
        };
        let Decl::Extension(ext) = d.as_ref() else {
            continue;
        };
        let Some(label) = extension_target_label(&ext.target, interner) else {
            continue;
        };
        for member in &ext.members {
            let name: Arc<str> = match member {
                ExtensionMember::Method(f) => {
                    Arc::from(format!("__ext_{label}_{}", interner.resolve(f.id)))
                }
                ExtensionMember::Getter { key, .. } => {
                    Arc::from(format!("__extget_{label}_{}", interner.resolve(*key)))
                }
                ExtensionMember::Setter { key, .. } => {
                    Arc::from(format!("__extset_{label}_{}", interner.resolve(*key)))
                }
            };
            out.insert(name);
        }
    }
}

pub(super) fn extension_target_label(
    t: &varn_core::ast::types::TypeNode,
    interner: &AtomInterner,
) -> Option<Arc<str>> {
    use varn_core::TypeKind;
    match &t.kind {
        TypeKind::Named(n, _) => Some(Arc::from(interner.resolve(*n))),
        TypeKind::Generic(n, _, _) => Some(Arc::from(interner.resolve(*n))),
        k @ (TypeKind::Primitive(_) | TypeKind::Builtin(_) | TypeKind::Literal(_)) => {
            k.lang_name().map(Arc::from)
        }
        TypeKind::Array(_) => Some(Arc::from("Array")),
        _ => None,
    }
}

pub(super) fn emit_extensions(
    program: &Program,
    ast_arena: &AstArena,
    ctx: &MCtx,
    expr_table: &FxHashMap<AstId, TypeEntry>,
    types: &mut TyTable,
    signatures: &mut Vec<Signature>,
    out: &mut Vec<TirFunction>,
) {
    use varn_core::ast::ExtensionMember;
    for &stmt in &program.body {
        let StmtKind::Decl(d) = &ast_arena.stmt(stmt).kind else {
            continue;
        };
        let Decl::Extension(ext) = d.as_ref() else {
            continue;
        };
        let Some(label) = extension_target_label(&ext.target, ctx.interner) else {
            continue;
        };
        let recv_ty = match label.as_ref() {
            "str" => BackendTy::Str,
            "int" => BackendTy::Int,
            "float" => BackendTy::Float,
            "bool" => BackendTy::Bool,
            "char" => BackendTy::Char,
            other => ctx
                .names
                .class_id(other)
                .map(BackendTy::Class)
                .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated)),
        };
        let this_cid = match recv_ty {
            BackendTy::Class(cid) => Some(cid),
            _ => None,
        };
        for member in &ext.members {
            let (mangled, params, body): (Arc<str>, Vec<Arc<str>>, StmtId) = match member {
                ExtensionMember::Method(f) => (
                    Arc::from(format!("__ext_{label}_{}", ctx.interner.resolve(f.id))),
                    f.params
                        .iter()
                        .map(|p| param_name(p, ctx.interner))
                        .collect(),
                    f.body,
                ),
                ExtensionMember::Getter { key, body, .. } => (
                    Arc::from(format!("__extget_{label}_{}", ctx.interner.resolve(*key))),
                    vec![],
                    *body,
                ),
                ExtensionMember::Setter {
                    key, param, body, ..
                } => (
                    Arc::from(format!("__extset_{label}_{}", ctx.interner.resolve(*key))),
                    vec![param_name(param, ctx.interner)],
                    *body,
                ),
            };
            let arity = params.len();
            let sig = fresh_sig(signatures, arity);
            let base = out.len() as u32 + 1;
            let mut mcls: Vec<TirFunction> = Vec::new();
            let (body_stmts, locals) = {
                let mut em = FnEmitter::new(
                    ast_arena,
                    expr_table,
                    types,
                    ctx.as_module_ctx(),
                    signatures,
                    &mut mcls,
                    base,
                    params,
                );
                if let Some(cid) = this_cid {
                    em = em.with_this(cid);
                }
                let b = match &ast_arena.stmt(body).kind {
                    StmtKind::Block { stmts } => em.lower_block(stmts),
                    _ => em.lower_block(std::slice::from_ref(&body)),
                };
                (b, std::mem::take(&mut em.locals))
            };
            out.push(TirFunction {
                name: mangled,
                sig,
                params: vec![BackendTy::Dynamic(DynReason::Unannotated); arity],
                return_ty: BackendTy::Dynamic(DynReason::Unannotated),
                locals,
                body: body_stmts,
                has_this: true,
                this_class: this_cid,
                is_async: false,
                is_generator: false,
                has_rest: matches!(member, ExtensionMember::Method(f) if f.params.last().is_some_and(|p| p.is_rest)),
            });
            out.extend(mcls);
        }
    }
}
