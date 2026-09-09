//! The checker emits TIR.
//!
//! The stage-2 replacement for `checker_annotations/`: instead of walking the
//! AST and noting types in a side map, it builds a `varn_tir::TirModule`
//! whose every node carries its type and resolution as mandatory fields. See
//! `docs/TIR_ETAPA_2_PLAN.md`.
//!
//! The whole executable AST lowers: literals, `Var` (local / param / global /
//! upvalue), every operator, member and index access, calls (direct / vtable /
//! by-name), `new`, enum construction and matching, collections, closures,
//! `async` / generators, all loop forms, `switch`, `try`, destructuring,
//! templates. A construct with no precise TIR shape (a host intrinsic, a
//! spread the arity rule can't see, an iterator-protocol `for…of`) still emits
//! real nodes typed `Dynamic(Unannotated)` — never a bare hole.

mod body;
mod tables;
mod ty;

pub use ty::{lower_type, NameResolver, NoNames};

use crate::binder::BindResult;
use crate::checker::TypeEntry;
use body::FnEmitter;
use rustc_hash::{FxHashMap, FxHashSet};
use std::rc::Rc;
use varn_core::ast::{
    AstId, Decl, ExportDecl, FunctionDecl, Param, Pattern, Program, Stmt, StmtKind,
};
use varn_core::TypeKind;
use varn_tir::{
    BackendTy, DynReason, FnId, Resolution, Signature, SigId, Span, TirExpr, TirExprKind,
    TirFunction, TirModule, TirObjectEntry, TirStmt, TyTable,
};

/// Build the TIR for one module from the same four inputs
/// `collect_type_annotations` consumes. Nothing else: a datum the checker does
/// not expose here is a gap in the checker, to be closed there.
pub fn emit_module(
    program: &Program,
    bind: &BindResult,
    expr_table: &FxHashMap<AstId, TypeEntry>,
    call_mappings: &FxHashMap<AstId, Vec<Option<usize>>>,
    ext_calls: &FxHashMap<u32, Rc<str>>,
    ext_members: &FxHashMap<u32, Rc<str>>,
    ext_set_members: &FxHashMap<u32, Rc<str>>,
) -> TirModule {
    let mut types = TyTable::default();
    ty::prime(&mut types);

    let tables::Tables { classes, enums, mut signatures, names } = tables::build(bind, &mut types);

    // Module value symbols → global slots, in binder declaration order.
    // Names this file itself declares at the top level — the only ones that
    // become a module-qualified global. A builtin (`print`) or a name reaching
    // us from the prelude is NOT one of these; it resolves by bare name.
    let mut declared: FxHashSet<Rc<str>> = FxHashSet::default();
    for stmt in &program.body {
        if let StmtKind::Decl(d) = &stmt.kind {
            collect_decl_names(d, &mut declared);
        }
    }

    let mut global_slots: FxHashMap<Rc<str>, u32> = FxHashMap::default();
    let mut globals: Vec<BackendTy> = Vec::new();
    for sym in bind.global_symbols() {
        if !is_value_symbol(sym.kind) {
            continue;
        }
        if !declared.contains(&sym.name) {
            continue;
        }
        if global_slots.contains_key(&sym.name) {
            continue;
        }
        let slot = globals.len() as u32;
        globals.push(
            sym.ty
                .as_ref()
                .map(|t| lower_type(t, &mut types, &names))
                .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated)),
        );
        global_slots.insert(sym.name.clone(), slot);
    }
    let mut global_names: Vec<Rc<str>> = vec![Rc::from(""); globals.len()];
    for (name, &slot) in &global_slots {
        global_names[slot as usize] = name.clone();
    }

    // Free functions, in declaration order: FnId is the index, arity is the
    // parameter count (the signature is built to match, so the verifier's
    // arity check agrees).
    let free_fns: Vec<&FunctionDecl> = program
        .body
        .iter()
        .filter_map(|s| match &s.kind {
            StmtKind::Decl(d) => free_function(d),
            _ => None,
        })
        .collect();
    let mut fn_index: FxHashMap<Rc<str>, (u32, u32)> = FxHashMap::default();
    for (i, f) in free_fns.iter().enumerate() {
        fn_index.entry(f.id.clone()).or_insert((i as u32, f.params.len() as u32));
    }

    let ctx = MCtx {
        names: &names,
        classes: &classes,
        enums: &enums,
        globals: &global_slots,
        fns: &fn_index,
        call_mappings,
        ext_calls,
        ext_members,
        ext_set_members,
    };

    // `functions` holds the free functions at indices 0..N (matching
    // `fn_index`), then every closure body, then class methods. A closure's
    // FnId is `n_free + <its position among all closures>`.
    let n_free = free_fns.len() as u32;
    let mut functions: Vec<TirFunction> = Vec::new();
    let mut closures: Vec<TirFunction> = Vec::new();

    // A closure's FnId is `n_free + <its absolute index in `closures`>`, and
    // `functions.extend(closures)` later places `closures[i]` at exactly that
    // index, so every free function and the module top level share this base.
    for f in &free_fns {
        let tf = emit_function(
            f, bind, expr_table, &mut types, &ctx, &mut signatures, &mut closures, n_free,
        );
        functions.push(tf);
    }

    // `namespace NS { export function f … }` — each member function lowers to a
    // closure body; the namespace itself becomes a plain object global built at
    // the declaration's source position (below, in the top-level loop).
    let mut ns_members: FxHashMap<Rc<str>, Vec<(Rc<str>, u32)>> = FxHashMap::default();
    for stmt in &program.body {
        let StmtKind::Decl(d) = &stmt.kind else { continue };
        let Some(ns) = namespace_decl(d) else { continue };
        let mut members = Vec::new();
        for m in &ns.body {
            let inner = match m {
                Decl::Export(ExportDecl::Decl { declaration, .. }) => declaration.as_ref(),
                other => other,
            };
            let Decl::Function(f) = inner else { continue };
            let tf = emit_function(
                f, bind, expr_table, &mut types, &ctx, &mut signatures, &mut closures, n_free,
            );
            let fnid = n_free + closures.len() as u32;
            closures.push(tf);
            members.push((f.id.clone(), fnid));
        }
        ns_members.insert(ns.id.clone(), members);
    }

    // Module top level: every statement, plus module-level `let` / `const`.
    let tl_base = n_free;
    let mut top_body = Vec::new();
    let top_locals = {
        let mut top = FnEmitter::new(
            expr_table,
            &mut types,
            ctx.as_module_ctx(),
            &mut signatures,
            &mut closures,
            tl_base,
            vec![],
        )
        .as_top_level();
        let mut class_ord: u32 = 0;
        for stmt in &program.body {
            match &stmt.kind {
                // A class / enum declaration: a `BuildClass` at this position,
                // in the same order `class_defs` is filled below.
                StmtKind::Decl(d)
                    if class_decl(d).is_some() || enum_decl(d).is_some() =>
                {
                    top_body.push(TirStmt::BuildClass(class_ord));
                    class_ord += 1;
                }
                StmtKind::Decl(d) if namespace_decl(d).is_some() => {
                    let ns = namespace_decl(d).unwrap();
                    if let (Some(&slot), Some(members)) =
                        (global_slots.get(ns.id.as_ref()), ns_members.get(&ns.id))
                    {
                        let dyno = || BackendTy::Dynamic(DynReason::Unannotated);
                        let entries = members
                            .iter()
                            .map(|(name, fnid)| TirObjectEntry::Field {
                                name: name.clone(),
                                value: TirExpr {
                                    kind: TirExprKind::Closure {
                                        func: FnId(*fnid),
                                        upvalues: vec![],
                                    },
                                    ty: dyno(),
                                    res: Resolution::None,
                                    span: Span::EMPTY,
                                },
                            })
                            .collect();
                        let obj = TirExpr {
                            kind: TirExprKind::ObjectLit { entries },
                            ty: dyno(),
                            res: Resolution::None,
                            span: Span::EMPTY,
                        };
                        let target = TirExpr {
                            kind: TirExprKind::Var,
                            ty: dyno(),
                            res: Resolution::GlobalSlot(slot),
                            span: Span::EMPTY,
                        };
                        top_body.push(TirStmt::Expr(TirExpr {
                            kind: TirExprKind::Assign {
                                target: Box::new(target),
                                value: Box::new(obj),
                            },
                            ty: BackendTy::Void,
                            res: Resolution::None,
                            span: Span::EMPTY,
                        }));
                    }
                }
                StmtKind::Decl(d) if variable_decl(d).is_none() => {}
                _ => top_body.extend(top.lower_stmt_as_block(stmt)),
            }
        }
        std::mem::take(&mut top.locals)
    };
    let top_level = TirFunction {
        name: Rc::from("<module>"),
        sig: SigId(0),
        params: vec![],
        return_ty: BackendTy::Void,
        locals: top_locals,
        body: top_body,
        has_this: false,
        this_class: None,
        // Module top level permits top-level `await`.
        is_async: true,
        is_generator: false,
    };

    functions.extend(closures);

    // Class / enum construction, methods and constructors, after every
    // closure. In source order, so a class can extend one declared earlier.
    let mut class_defs: Vec<varn_tir::TirClassDef> = Vec::new();
    for stmt in &program.body {
        let StmtKind::Decl(decl) = &stmt.kind else { continue };
        if let Some(class) = class_decl(decl) {
            class_defs.push(emit_class(
                class, &ctx, expr_table, &mut types, &mut signatures, &mut functions,
            ));
        } else if let Some(en) = enum_decl(decl) {
            class_defs.push(emit_enum(
                en, &ctx, expr_table, &mut types, &mut signatures, &mut functions,
            ));
        }
    }

    emit_extensions(program, &ctx, expr_table, &mut types, &mut signatures, &mut functions);

    let imports = collect_imports(program);
    let exports = collect_exports(program);

    TirModule {
        source_file: Rc::from(program.filename.as_ref()),
        imports,
        exports,
        types,
        classes,
        enums,
        signatures,
        functions,
        globals,
        global_names,
        class_defs,
        top_level,
    }
}

/// Owns the borrows a `ModuleCtx` bundles, so the many emit helpers take one
/// `&MCtx` instead of four separate references.
struct MCtx<'a> {
    names: &'a tables::NameIndex,
    classes: &'a [varn_tir::ClassInfo],
    enums: &'a [varn_tir::EnumInfo],
    globals: &'a FxHashMap<Rc<str>, u32>,
    fns: &'a FxHashMap<Rc<str>, (u32, u32)>,
    call_mappings: &'a FxHashMap<AstId, Vec<Option<usize>>>,
    ext_calls: &'a FxHashMap<u32, Rc<str>>,
    ext_members: &'a FxHashMap<u32, Rc<str>>,
    ext_set_members: &'a FxHashMap<u32, Rc<str>>,
}

impl<'a> MCtx<'a> {
    fn as_module_ctx(&self) -> body::ModuleCtx<'a> {
        body::ModuleCtx {
            names: self.names,
            classes: self.classes,
            enums: self.enums,
            globals: self.globals,
            fns: self.fns,
            call_mappings: self.call_mappings,
            ext_calls: self.ext_calls,
            ext_members: self.ext_members,
            ext_set_members: self.ext_set_members,
        }
    }
}

fn is_value_symbol(kind: crate::symbol::SymbolKind) -> bool {
    use crate::symbol::SymbolKind as K;
    matches!(
        kind,
        K::Var | K::Let | K::Const | K::Function | K::Class | K::Enum | K::Namespace | K::Struct
    )
}

fn free_function(decl: &Decl) -> Option<&FunctionDecl> {
    match decl {
        Decl::Function(f) => Some(f),
        Decl::Export(ExportDecl::Decl { declaration, .. }) => match declaration.as_ref() {
            Decl::Function(f) => Some(f),
            _ => None,
        },
        _ => None,
    }
}

fn namespace_decl(decl: &Decl) -> Option<&varn_core::ast::NamespaceDecl> {
    match decl {
        Decl::Namespace(n) => Some(n),
        Decl::Export(ExportDecl::Decl { declaration, .. }) => match declaration.as_ref() {
            Decl::Namespace(n) => Some(n),
            _ => None,
        },
        _ => None,
    }
}

fn variable_decl(decl: &Decl) -> Option<&varn_core::ast::VariableDecl> {
    match decl {
        Decl::Variable(v) => Some(v),
        Decl::Export(ExportDecl::Decl { declaration, .. }) => match declaration.as_ref() {
            Decl::Variable(v) => Some(v),
            _ => None,
        },
        _ => None,
    }
}

fn class_decl(decl: &Decl) -> Option<&varn_core::ast::ClassDecl> {
    match decl {
        Decl::Class(c) => Some(c),
        Decl::Export(ExportDecl::Decl { declaration, .. }) => match declaration.as_ref() {
            Decl::Class(c) => Some(c),
            _ => None,
        },
        _ => None,
    }
}

/// Top-level binding names this declaration introduces — functions, classes,
/// enums, `let`/`const` (including destructured), namespaces, and import
/// locals. The set the module qualifies its globals by.
fn collect_decl_names(decl: &Decl, out: &mut FxHashSet<Rc<str>>) {
    use varn_core::ast::{ImportSpecifier, Pattern as P};
    fn pat_names(p: &P, out: &mut FxHashSet<Rc<str>>) {
        match p {
            P::Identifier { name, .. } => {
                out.insert(name.clone());
            }
            P::Array { elements, rest, .. } => {
                for e in elements.iter().flatten() {
                    pat_names(&e.pattern, out);
                }
                if let Some(r) = rest {
                    pat_names(r, out);
                }
            }
            P::Object { properties, rest, .. } => {
                for prop in properties {
                    pat_names(&prop.value, out);
                }
                if let Some(r) = rest {
                    pat_names(r, out);
                }
            }
            P::Assignment { left, .. } => pat_names(left, out),
            P::Rest { argument, .. } => pat_names(argument, out),
            _ => {}
        }
    }
    match decl {
        Decl::Function(f) => {
            out.insert(f.id.clone());
        }
        Decl::Class(c) => {
            if let Some(id) = &c.id {
                out.insert(id.clone());
            }
        }
        Decl::Enum(e) => {
            out.insert(e.id.clone());
        }
        Decl::Variable(v) => {
            for d in &v.declarators {
                pat_names(&d.id, out);
            }
        }
        Decl::Namespace(ns) => {
            out.insert(ns.id.clone());
        }
        Decl::Import(i) => {
            for spec in &i.specifiers {
                let (ImportSpecifier::Default { local, .. }
                | ImportSpecifier::Named { local, .. }
                | ImportSpecifier::Namespace { local, .. }) = spec;
                out.insert(local.clone());
            }
        }
        Decl::Export(ExportDecl::Decl { declaration, .. }) => collect_decl_names(declaration, out),
        _ => {}
    }
}

/// The type label an `extension X on T` targets — matches the binder's
/// mangling (`__ext_{label}_{name}`).
fn extension_target_label(t: &varn_core::ast::types::TypeNode) -> Option<Rc<str>> {
    use varn_core::TypeKind;
    match &t.kind {
        TypeKind::Named(n, _) => Some(Rc::from(n.as_str())),
        TypeKind::Generic(n, _, _) => Some(Rc::from(n.as_str())),
        TypeKind::Intrinsic(tag) => Some(Rc::from(varn_core::IntrinsicType::from(*tag).as_str())),
        TypeKind::Array(_) => Some(Rc::from("Array")),
        _ => None,
    }
}

fn emit_extensions(
    program: &Program,
    ctx: &MCtx,
    expr_table: &FxHashMap<AstId, TypeEntry>,
    types: &mut TyTable,
    signatures: &mut Vec<Signature>,
    out: &mut Vec<TirFunction>,
) {
    use varn_core::ast::ExtensionMember;
    for stmt in &program.body {
        let StmtKind::Decl(d) = &stmt.kind else { continue };
        let Decl::Extension(ext) = d.as_ref() else { continue };
        let Some(label) = extension_target_label(&ext.target) else { continue };
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
            let (mangled, params, body): (Rc<str>, Vec<Rc<str>>, &Stmt) = match member {
                ExtensionMember::Method(f) => (
                    Rc::from(format!("__ext_{label}_{}", f.id)),
                    f.params.iter().map(param_name).collect(),
                    &f.body,
                ),
                ExtensionMember::Getter { key, body, .. } => {
                    (Rc::from(format!("__extget_{label}_{key}")), vec![], body)
                }
                ExtensionMember::Setter { key, param, body, .. } => (
                    Rc::from(format!("__extset_{label}_{key}")),
                    vec![param_name(param)],
                    body,
                ),
            };
            let arity = params.len();
            let sig = fresh_sig(signatures, arity);
            let base = out.len() as u32 + 1;
            let mut mcls: Vec<TirFunction> = Vec::new();
            let (body_stmts, locals) = {
                let mut em = FnEmitter::new(
                    expr_table, types, ctx.as_module_ctx(), signatures, &mut mcls, base, params,
                );
                if let Some(cid) = this_cid {
                    em = em.with_this(cid);
                }
                let b = match &body.kind {
                    StmtKind::Block { stmts } => em.lower_block(stmts),
                    _ => em.lower_block(std::slice::from_ref(body)),
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
            });
            out.extend(mcls);
        }
    }
}

/// `this.<field> = <param i>` — a parameter property / primary-constructor
/// field assignment.
fn param_field_assign(field: Rc<str>, param: u32) -> TirStmt {
    this_field_assign(
        field,
        TirExpr {
            kind: TirExprKind::Var,
            ty: BackendTy::Dynamic(DynReason::Unannotated),
            res: Resolution::Param(param),
            span: Span::EMPTY,
        },
    )
}

/// `this.<field> = <value>` as a statement.
fn this_field_assign(field: Rc<str>, value: TirExpr) -> TirStmt {
    let this = TirExpr {
        kind: TirExprKind::Var,
        ty: BackendTy::Dynamic(DynReason::Unannotated),
        res: Resolution::None,
        span: Span::EMPTY,
    };
    TirStmt::Expr(TirExpr {
        kind: TirExprKind::Assign {
            target: Box::new(TirExpr {
                kind: TirExprKind::Field { object: Box::new(this), name: field },
                ty: BackendTy::Dynamic(DynReason::Unannotated),
                res: Resolution::None,
                span: Span::EMPTY,
            }),
            value: Box::new(value),
        },
        ty: BackendTy::Void,
        res: Resolution::None,
        span: Span::EMPTY,
    })
}

fn collect_exports(program: &Program) -> Vec<varn_tir::TirExport> {
    let mut out = Vec::new();
    let mut push = |exported: Rc<str>, local: Rc<str>, from: Option<Rc<str>>, ns: bool| {
        out.push(varn_tir::TirExport { exported, local, reexport_from: from, namespace: ns });
    };
    for stmt in &program.body {
        let StmtKind::Decl(d) = &stmt.kind else { continue };
        match d.as_ref() {
            Decl::Export(ExportDecl::Decl { declaration, .. }) => {
                let mut names = FxHashSet::default();
                collect_decl_names(declaration, &mut names);
                for n in names {
                    push(n.clone(), n, None, false);
                }
            }
            Decl::Export(ExportDecl::Named { specifiers, source, .. }) => {
                for sp in specifiers {
                    push(sp.exported.clone(), sp.local.clone(), source.clone(), false);
                }
            }
            Decl::Export(ExportDecl::All { source, alias: Some(alias), .. }) => {
                push(alias.clone(), alias.clone(), Some(source.clone()), true);
            }
            Decl::Export(ExportDecl::Default { .. }) => {
                push(Rc::from("default"), Rc::from("default"), None, false);
            }
            _ => {}
        }
    }
    out
}

fn collect_imports(program: &Program) -> Vec<varn_tir::TirImport> {
    use varn_core::ast::ImportSpecifier as IS;
    let mut out = Vec::new();
    for stmt in &program.body {
        let StmtKind::Decl(d) = &stmt.kind else { continue };
        let Decl::Import(imp) = d.as_ref() else { continue };
        let specs = imp
            .specifiers
            .iter()
            .map(|s| {
                let (local, kind) = match s {
                    IS::Default { local, .. } => (local.clone(), varn_tir::TirImportKind::Default),
                    IS::Namespace { local, .. } => {
                        (local.clone(), varn_tir::TirImportKind::Namespace)
                    }
                    IS::Named { local, imported, .. } => (
                        local.clone(),
                        varn_tir::TirImportKind::Named(imported.clone()),
                    ),
                };
                varn_tir::TirImportSpec { local, kind }
            })
            .collect();
        out.push(varn_tir::TirImport {
            source: imp.source.clone(),
            is_type_only: imp.is_type,
            specs,
        });
    }
    out
}

fn enum_decl(decl: &Decl) -> Option<&varn_core::ast::EnumDecl> {
    match decl {
        Decl::Enum(e) => Some(e),
        Decl::Export(ExportDecl::Decl { declaration, .. }) => match declaration.as_ref() {
            Decl::Enum(e) => Some(e),
            _ => None,
        },
        _ => None,
    }
}

/// Emit one member body (method, constructor, accessor, static block) as a
/// `TirFunction`, push it (and any closures it spawns) onto `out`, and return
/// its `FnId`.
#[allow(clippy::too_many_arguments)]
fn emit_member_fn(
    display_name: Rc<str>,
    params: &[Param],
    body: &Stmt,
    is_async: bool,
    is_generator: bool,
    this_class: Option<varn_tir::ClassId>,
    this_enum: Option<varn_tir::EnumId>,
    sig: SigId,
    ctx: &MCtx,
    expr_table: &FxHashMap<AstId, TypeEntry>,
    types: &mut TyTable,
    signatures: &mut Vec<Signature>,
    out: &mut Vec<TirFunction>,
) -> varn_tir::FnId {
    let sig_snapshot = signatures[sig.0 as usize].clone();
    let param_names: Vec<Rc<str>> = params.iter().map(param_name).collect();
    let fn_id = varn_tir::FnId(out.len() as u32);
    // Reserve this slot; the member's own closures follow it.
    let base = out.len() as u32 + 1;
    let mut mcls: Vec<TirFunction> = Vec::new();
    let (body_stmts, locals) = {
        let mut em = FnEmitter::new(
            expr_table, types, ctx.as_module_ctx(), signatures, &mut mcls, base, param_names,
        );
        if let Some(cid) = this_class {
            em = em.with_this(cid);
        }
        if let Some(eid) = this_enum {
            em = em.with_this_enum(eid);
        }
        let mut b = em.destructure_params(params);
        // TypeScript-style parameter properties: `constructor(public id: int)`
        // implies `this.id = id`.
        for (i, p) in params.iter().enumerate() {
            if p.modifiers.visibility.is_some() || p.modifiers.is_readonly {
                if let Pattern::Identifier { name, .. } = &p.pattern {
                    b.push(param_field_assign(name.clone(), i as u32));
                }
            }
        }
        b.extend(match &body.kind {
            StmtKind::Block { stmts } => em.lower_block(stmts),
            _ => em.lower_block(std::slice::from_ref(body)),
        });
        (b, std::mem::take(&mut em.locals))
    };
    out.push(TirFunction {
        name: display_name,
        sig,
        params: sig_snapshot.params,
        return_ty: sig_snapshot.return_ty,
        locals,
        body: body_stmts,
        has_this: this_class.is_some() || this_enum.is_some(),
        this_class,
        is_async,
        is_generator,
    });
    out.extend(mcls);
    fn_id
}

fn fresh_sig(signatures: &mut Vec<Signature>, arity: usize) -> SigId {
    let id = SigId(signatures.len() as u32);
    signatures.push(Signature {
        params: vec![BackendTy::Dynamic(DynReason::Unannotated); arity],
        return_ty: BackendTy::Dynamic(DynReason::Unannotated),
    });
    id
}

fn lower_outer(
    e: &varn_core::ast::Expr,
    ctx: &MCtx,
    expr_table: &FxHashMap<AstId, TypeEntry>,
    types: &mut TyTable,
    signatures: &mut Vec<Signature>,
    closures: &mut Vec<TirFunction>,
    base: u32,
    this_class: Option<varn_tir::ClassId>,
) -> (Vec<TirStmt>, TirExpr) {
    let mut em = FnEmitter::new(
        expr_table, types, ctx.as_module_ctx(), signatures, closures, base, vec![],
    );
    if let Some(cid) = this_class {
        em = em.with_this(cid);
    }
    em.lower_outer_expr(e)
}

fn emit_class(
    class: &varn_core::ast::ClassDecl,
    ctx: &MCtx,
    expr_table: &FxHashMap<AstId, TypeEntry>,
    types: &mut TyTable,
    signatures: &mut Vec<Signature>,
    out: &mut Vec<TirFunction>,
) -> varn_tir::TirClassDef {
    use varn_core::ast::ClassMember;
    let Some(class_name) = class.id.clone() else {
        return varn_tir::TirClassDef::default();
    };
    let class_id = ctx.names.class_id(&class_name);

    let mut def = varn_tir::TirClassDef {
        name: class_name.clone(),
        class_id,
        ..Default::default()
    };

    if let Some(sup) = &class.super_class {
        if let varn_core::ast::ExprKind::Identifier { name } = &sup.kind {
            def.parent = ctx.names.class_id(name);
        }
        let (pre, x) =
            lower_outer(sup, ctx, expr_table, types, signatures, out, out.len() as u32, None);
        def.prelude.extend(pre);
        def.super_class = Some(x);
    }
    for deco in &class.decorators {
        let (pre, x) = lower_outer(
            &deco.expression, ctx, expr_table, types, signatures, out, out.len() as u32, class_id,
        );
        def.prelude.extend(pre);
        def.decorators.push(x);
    }

    let info_sig = |key: &str, arity: usize, signatures: &mut Vec<Signature>| -> SigId {
        class_id
            .and_then(|cid| {
                let info = &ctx.classes[cid.0 as usize];
                info.method_slot(key).and_then(|s| info.method_at(s)).map(|e| e.sig)
            })
            .unwrap_or_else(|| fresh_sig(signatures, arity))
    };

    // Instance-field defaults: `this.<field> = <init>` prepended to the
    // constructor (a synthetic one if the class declares none).
    let field_defaults: Vec<TirStmt> = {
        let mut stmts = Vec::new();
        let base = out.len() as u32;
        let mut cls: Vec<TirFunction> = Vec::new();
        let mut em = FnEmitter::new(
            expr_table, types, ctx.as_module_ctx(), signatures, &mut cls, base, vec![],
        );
        if let Some(cid) = class_id {
            em = em.with_this(cid);
        }
        for member in &class.body {
            if let ClassMember::Property { key, init: Some(init), modifiers, .. } = member {
                if modifiers.is_static {
                    continue;
                }
                let value = em.lower_expression(init);
                stmts.append(&mut em.take_pending());
                stmts.push(this_field_assign(key.clone(), value));
            }
        }
        drop(em);
        out.extend(cls);
        stmts
    };

    for member in &class.body {
        match member {
            ClassMember::Constructor { params, body, .. } => {
                let sig = info_sig("constructor", params.len(), signatures);
                let id = emit_member_fn(
                    Rc::from(format!("{class_name}.constructor")),
                    params, body, false, false, class_id, None, sig, ctx, expr_table, types,
                    signatures, out,
                );
                def.methods.push(varn_tir::TirClassMember {
                    key: Rc::from("constructor"),
                    func: id,
                    is_static: false,
                    is_private: false,
                    decorators: vec![],
                    });
            }
            ClassMember::Method {
                key, params, body: Some(body), modifiers, decorators, ..
            } => {
                let sig = info_sig(key, params.len(), signatures);
                let id = emit_member_fn(
                    Rc::from(format!("{class_name}.{key}")),
                    params,
                    body,
                    modifiers.is_async,
                    modifiers.is_generator,
                    (!modifiers.is_static).then_some(()).and(class_id),
                    None,
                    sig,
                    ctx,
                    expr_table,
                    types,
                    signatures,
                    out,
                );
                let decos: Vec<TirExpr> = decorators
                    .iter()
                    .map(|d| {
                        let (pre, x) = lower_outer(
                            &d.expression, ctx, expr_table, types, signatures, out,
                            out.len() as u32, class_id,
                        );
                        def.prelude.extend(pre);
                        x
                    })
                    .collect();
                def.methods.push(varn_tir::TirClassMember {
                    key: key.clone(),
                    func: id,
                    is_static: modifiers.is_static,
                    is_private: matches!(modifiers.visibility, Some(varn_core::ast::operators::Visibility::Private)),
                    decorators: decos,
                });
            }
            ClassMember::Getter { key, body: Some(body), modifiers, .. } => {
                let sig = fresh_sig(signatures, 0);
                let id = emit_member_fn(
                    Rc::from(format!("{class_name}.get {key}")),
                    &[], body, false, false,
                    (!modifiers.is_static).then_some(()).and(class_id),
                    None,
                    sig, ctx, expr_table, types, signatures, out,
                );
                def.accessors.push(varn_tir::TirClassAccessor {
                    key: key.clone(),
                    func: id,
                    is_getter: true,
                    is_static: modifiers.is_static,
                });
            }
            ClassMember::Setter { key, param, body: Some(body), modifiers, .. } => {
                let sig = fresh_sig(signatures, 1);
                let ps = std::slice::from_ref(param);
                let id = emit_member_fn(
                    Rc::from(format!("{class_name}.set {key}")),
                    ps, body, false, false,
                    (!modifiers.is_static).then_some(()).and(class_id),
                    None,
                    sig, ctx, expr_table, types, signatures, out,
                );
                def.accessors.push(varn_tir::TirClassAccessor {
                    key: key.clone(),
                    func: id,
                    is_getter: false,
                    is_static: modifiers.is_static,
                });
            }
            ClassMember::Property { key, init, modifiers, .. } if modifiers.is_static => {
                let init_x = init.as_ref().map(|e| {
                    let (pre, x) = lower_outer(
                        e, ctx, expr_table, types, signatures, out, out.len() as u32, class_id,
                    );
                    def.prelude.extend(pre);
                    x
                });
                def.statics.push((key.clone(), init_x));
            }
            ClassMember::StaticBlock { body, .. } => {
                let sig = fresh_sig(signatures, 0);
                let id = emit_member_fn(
                    Rc::from(format!("{class_name}.<static>")),
                    &[], body, false, false, class_id, None, sig, ctx, expr_table, types, signatures,
                    out,
                );
                def.static_blocks.push(id);
            }
            _ => {}
        }
    }

    // C# / Kotlin primary constructor: `class User(public id: int, …)` — the
    // params are fields, assigned `this.id = id` in a synthesized constructor.
    let primary: &[Param] = class.primary_params.as_deref().unwrap_or(&[]);
    let has_ctor = def.methods.iter().any(|m| m.key.as_ref() == "constructor");

    if !field_defaults.is_empty() || (!primary.is_empty() && !has_ctor) {
        match def.methods.iter().find(|m| m.key.as_ref() == "constructor") {
            Some(ctor) => {
                // Prepend the defaults; an explicit `this.x = arg` later just
                // overwrites, matching field-then-constructor order.
                let body = &mut out[ctor.func.0 as usize].body;
                let mut new = field_defaults;
                new.extend(std::mem::take(body));
                *body = new;
            }
            None => {
                // A synthetic constructor: primary params (with their defaults,
                // via `destructure_params`), then the field defaults, then
                // `this.<p> = p` for every primary param.
                let empty_body = Stmt {
                    id: class.ast_id,
                    range: class.range.clone(),
                    kind: StmtKind::Block { stmts: vec![] },
                };
                let sig = fresh_sig(signatures, primary.len());
                {
                    let tys: Vec<BackendTy> = primary
                        .iter()
                        .map(|p| {
                            p.type_ann
                                .as_ref()
                                .map(|t| {
                                    lower_type(
                                        &crate::binder::resolve_type_node(t, None),
                                        types,
                                        ctx.names,
                                    )
                                })
                                .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated))
                        })
                        .collect();
                    signatures[sig.0 as usize].params = tys;
                }
                let id = emit_member_fn(
                    Rc::from(format!("{class_name}.constructor")),
                    primary, &empty_body, false, false, class_id, None, sig, ctx, expr_table,
                    types, signatures, out,
                );
                let body = &mut out[id.0 as usize].body;
                // `this.<p> = p` for params `emit_member_fn` did not (those
                // without a visibility / readonly modifier). Appended after the
                // default-value checks it emitted.
                for (i, p) in primary.iter().enumerate() {
                    if p.modifiers.visibility.is_some() || p.modifiers.is_readonly {
                        continue;
                    }
                    if let Pattern::Identifier { name, .. } = &p.pattern {
                        body.push(param_field_assign(name.clone(), i as u32));
                    }
                }
                let mut new = field_defaults;
                new.extend(std::mem::take(body));
                *body = new;
                def.methods.push(varn_tir::TirClassMember {
                    key: Rc::from("constructor"),
                    func: id,
                    is_static: false,
                    is_private: false,
                    decorators: vec![],
                });
            }
        }
    }

    def
}

fn emit_enum(
    en: &varn_core::ast::EnumDecl,
    ctx: &MCtx,
    expr_table: &FxHashMap<AstId, TypeEntry>,
    types: &mut TyTable,
    signatures: &mut Vec<Signature>,
    out: &mut Vec<TirFunction>,
) -> varn_tir::TirClassDef {
    use varn_core::ast::ClassMember;
    let name = en.id.clone();
    let enum_id = ctx.names.enum_id(&name);
    let mut def = varn_tir::TirClassDef {
        name: name.clone(),
        enum_id,
        ..Default::default()
    };

    let mut tag = 0i64;
    for m in &en.members {
        if let Some(init) = &m.init {
            if let varn_core::ast::ExprKind::IntLiteral { value, .. } = &init.kind {
                tag = *value;
            }
        }
        let fields_str = m
            .payload_fields
            .iter()
            .map(|f| f.name.as_ref())
            .collect::<Vec<&str>>()
            .join(",");
        let meta = if fields_str.is_empty() {
            format!("{name}.{}", m.id)
        } else {
            format!("{name}.{}:{fields_str}", m.id)
        };
        let mut const_args = Vec::new();
        for f in &m.payload_fields {
            if let Some(init) = &f.init {
                let (pre, x) = lower_outer(
                    init, ctx, expr_table, types, signatures, out, out.len() as u32, None,
                );
                def.prelude.extend(pre);
                const_args.push(x);
            }
        }
        def.variants.push(varn_tir::TirVariantDef {
            name: m.id.clone(),
            tag,
            meta: Rc::from(meta.as_str()),
            const_args,
        });
        tag += 1;
    }

    // Enums may carry methods / getters / setters in `body`, `this` typed as
    // the enum's class handle (the binder registers it under `classes` too).
    let this_cid = ctx.names.class_id(&name);
    for member in &en.body {
        match member {
            ClassMember::Method { key, params, body: Some(body), modifiers, .. } => {
                let sig = fresh_sig(signatures, params.len());
                let id = emit_member_fn(
                    Rc::from(format!("{name}.{key}")),
                    params,
                    body,
                    modifiers.is_async,
                    modifiers.is_generator,
                    (!modifiers.is_static).then_some(()).and(this_cid),
                    (!modifiers.is_static).then_some(()).and(enum_id),
                    sig, ctx, expr_table, types, signatures, out,
                );
                def.methods.push(varn_tir::TirClassMember {
                    key: key.clone(),
                    func: id,
                    is_static: modifiers.is_static,
                    is_private: false,
                    decorators: vec![],
                    });
            }
            ClassMember::Constructor { params, body, .. } => {
                let sig = fresh_sig(signatures, params.len());
                let id = emit_member_fn(
                    Rc::from(format!("{name}.constructor")),
                    params, body, false, false, this_cid, enum_id, sig, ctx, expr_table, types,
                    signatures, out,
                );
                def.methods.push(varn_tir::TirClassMember {
                    key: Rc::from("constructor"),
                    func: id,
                    is_static: false,
                    is_private: false,
                    decorators: vec![],
                    });
            }
            ClassMember::Getter { key, body: Some(body), modifiers, .. } => {
                let sig = fresh_sig(signatures, 0);
                let id = emit_member_fn(
                    Rc::from(format!("{name}.get {key}")),
                    &[], body, false, false,
                    (!modifiers.is_static).then_some(()).and(this_cid),
                    (!modifiers.is_static).then_some(()).and(enum_id),
                    sig, ctx, expr_table, types, signatures, out,
                );
                def.accessors.push(varn_tir::TirClassAccessor {
                    key: key.clone(),
                    func: id,
                    is_getter: true,
                    is_static: modifiers.is_static,
                });
            }
            ClassMember::Setter { key, param, body: Some(body), modifiers, .. } => {
                let sig = fresh_sig(signatures, 1);
                let id = emit_member_fn(
                    Rc::from(format!("{name}.set {key}")),
                    std::slice::from_ref(param), body, false, false,
                    (!modifiers.is_static).then_some(()).and(this_cid),
                    (!modifiers.is_static).then_some(()).and(enum_id),
                    sig, ctx, expr_table, types, signatures, out,
                );
                def.accessors.push(varn_tir::TirClassAccessor {
                    key: key.clone(),
                    func: id,
                    is_getter: false,
                    is_static: modifiers.is_static,
                });
            }
            _ => {}
        }
    }
    def
}

fn param_name(p: &Param) -> Rc<str> {
    match &p.pattern {
        Pattern::Identifier { name, .. } => name.clone(),
        _ => Rc::from("_"),
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_function(
    f: &FunctionDecl,
    bind: &BindResult,
    expr_table: &FxHashMap<AstId, TypeEntry>,
    types: &mut TyTable,
    ctx: &MCtx,
    signatures: &mut Vec<Signature>,
    closures: &mut Vec<TirFunction>,
    closure_base: u32,
) -> TirFunction {
    // The function's signature is on its global symbol as a `Fn` type. Its
    // arity is forced to the AST parameter count so the verifier's arity
    // check on a DirectFn call site agrees with `fn_index`.
    let sym_ty = bind
        .global_symbols()
        .find(|s| s.name == f.id)
        .and_then(|s| s.ty.clone());

    let arity = f.params.len();
    let mut param_tys = vec![BackendTy::Dynamic(DynReason::Unannotated); arity];
    let mut return_ty = BackendTy::Dynamic(DynReason::Unannotated);
    if let Some(TypeKind::Fn(ft)) = sym_ty.as_ref().map(|t| t.kind()) {
        for (i, p) in ft.params.iter().take(arity).enumerate() {
            param_tys[i] = lower_type(&p.ty, types, ctx.names);
        }
        return_ty = lower_type(&ft.return_type, types, ctx.names);
    }

    let sig = SigId(signatures.len() as u32);
    signatures.push(Signature { params: param_tys.clone(), return_ty });

    let param_names: Vec<Rc<str>> = f.params.iter().map(param_name).collect();
    let (body, locals) = {
        let mut em = FnEmitter::new(
            expr_table,
            types,
            ctx.as_module_ctx(),
            signatures,
            closures,
            closure_base,
            param_names,
        );
        let mut b = em.destructure_params(&f.params);
        b.extend(match &f.body.kind {
            StmtKind::Block { stmts } => em.lower_block(stmts),
            _ => em.lower_block(std::slice::from_ref(&f.body)),
        });
        (b, std::mem::take(&mut em.locals))
    };

    TirFunction {
        name: f.id.clone(),
        sig,
        params: param_tys,
        return_ty,
        locals,
        body,
        has_this: false,
        this_class: None,
        is_async: f.modifiers.is_async,
        is_generator: f.modifiers.is_generator,
    }
}

#[allow(dead_code)]
fn placeholder_stmt() -> TirStmt {
    TirStmt::Expr(TirExpr {
        kind: TirExprKind::NullLit,
        ty: BackendTy::Dynamic(DynReason::Unannotated),
        res: Resolution::None,
        span: Span::EMPTY,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use varn_tir::{verify_module, Coverage};

    fn stub_module() -> TirModule {
        TirModule {
            source_file: Rc::from("t.vn"),
            imports: vec![], exports: vec![],
            types: TyTable::default(),
            classes: vec![],
            enums: vec![],
            signatures: vec![Signature { params: vec![], return_ty: BackendTy::Void }],
            functions: vec![],
            globals: vec![], global_names: vec![],
            class_defs: vec![],
            top_level: TirFunction {
                name: Rc::from("<module>"),
                sig: SigId(0),
                params: vec![],
                return_ty: BackendTy::Void,
                locals: vec![],
                body: vec![placeholder_stmt()],
                has_this: false,
                this_class: None,
                is_async: false,
                is_generator: false,
            },
        }
    }

    #[test]
    fn a_placeholder_only_module_verifies() {
        let m = stub_module();
        assert!(verify_module(&m).is_ok(), "{:?}", verify_module(&m));
    }

    #[test]
    fn a_placeholder_counts_as_dynamic() {
        let c = Coverage::of(&stub_module());
        assert_eq!(c.dynamic_by_reason(DynReason::Unannotated), 1);
    }
}
