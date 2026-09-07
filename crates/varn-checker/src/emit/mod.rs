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
use crate::module_resolver::ImportResolver;
use body::FnEmitter;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::ast::{
    AstId, Decl, ExportDecl, FunctionDecl, Param, Pattern, Program, Stmt, StmtKind,
};
use varn_core::TypeKind;
use varn_tir::{
    BackendTy, DynReason, Resolution, Signature, SigId, Span, TirExpr, TirExprKind, TirFunction,
    TirModule, TirStmt, TyTable,
};

/// Build the TIR for one module from the same four inputs
/// `collect_type_annotations` consumes. Nothing else: a datum the checker does
/// not expose here is a gap in the checker, to be closed there.
pub fn emit_module(
    program: &Program,
    bind: &BindResult,
    _resolver: &dyn ImportResolver,
    expr_table: &FxHashMap<AstId, TypeEntry>,
) -> TirModule {
    let mut types = TyTable::default();
    ty::prime(&mut types);

    let tables::Tables { classes, enums, mut signatures, names } = tables::build(bind, &mut types);

    // Module value symbols → global slots, in binder declaration order.
    let mut global_slots: FxHashMap<Rc<str>, u32> = FxHashMap::default();
    let mut globals: Vec<BackendTy> = Vec::new();
    for sym in bind.global_symbols() {
        if !is_value_symbol(sym.kind) {
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
        for stmt in &program.body {
            match &stmt.kind {
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

    // Class methods and constructors, after every closure.
    for stmt in &program.body {
        let StmtKind::Decl(decl) = &stmt.kind else { continue };
        if let Some(class) = class_decl(decl) {
            emit_class_methods(class, &ctx, expr_table, &mut types, &mut signatures, &mut functions);
        }
    }

    TirModule {
        source_file: Rc::from(program.filename.as_ref()),
        types,
        classes,
        enums,
        signatures,
        functions,
        globals,
        global_names,
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
}

impl<'a> MCtx<'a> {
    fn as_module_ctx(&self) -> body::ModuleCtx<'a> {
        body::ModuleCtx {
            names: self.names,
            classes: self.classes,
            enums: self.enums,
            globals: self.globals,
            fns: self.fns,
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

fn emit_class_methods(
    class: &varn_core::ast::ClassDecl,
    ctx: &MCtx,
    expr_table: &FxHashMap<AstId, TypeEntry>,
    types: &mut TyTable,
    signatures: &mut Vec<Signature>,
    out: &mut Vec<TirFunction>,
) {
    use varn_core::ast::ClassMember;
    let Some(class_name) = class.id.as_ref() else { return };
    let Some(class_id) = ctx.names.class_id(class_name) else { return };
    let info = &ctx.classes[class_id.0 as usize];

    for member in &class.body {
        let (key, params, body, is_async, is_generator): (Rc<str>, &[Param], &Stmt, bool, bool) =
            match member {
                ClassMember::Method { key, params, body: Some(body), modifiers, .. } => (
                    key.clone(),
                    params.as_slice(),
                    body,
                    modifiers.is_async,
                    modifiers.is_generator,
                ),
                ClassMember::Constructor { params, body, .. } => {
                    (Rc::from("constructor"), params.as_slice(), body, false, false)
                }
                _ => continue,
            };

        // A method reuses its vtable signature; a constructor gets a fresh one
        // (constructors are not dispatched).
        let sig = match info.method_slot(&key).and_then(|s| info.method_at(s)) {
            Some(entry) => entry.sig,
            None => {
                let id = SigId(signatures.len() as u32);
                signatures.push(Signature {
                    params: vec![BackendTy::Dynamic(DynReason::Unannotated); params.len()],
                    return_ty: BackendTy::Dynamic(DynReason::Unannotated),
                });
                id
            }
        };
        let sig_snapshot = signatures[sig.0 as usize].clone();

        let param_names: Vec<Rc<str>> = params.iter().map(param_name).collect();
        // Reserve one slot for the method itself; its closures follow.
        let base = out.len() as u32 + 1;
        let mut mcls: Vec<TirFunction> = Vec::new();
        let (body_stmts, locals) = {
            let mut em = FnEmitter::new(
                expr_table,
                types,
                ctx.as_module_ctx(),
                signatures,
                &mut mcls,
                base,
                param_names,
            )
            .with_this(class_id);
            let mut b = em.destructure_params(params);
            b.extend(match &body.kind {
                StmtKind::Block { stmts } => em.lower_block(stmts),
                _ => em.lower_block(std::slice::from_ref(body)),
            });
            (b, std::mem::take(&mut em.locals))
        };

        out.push(TirFunction {
            name: Rc::from(format!("{class_name}.{key}")),
            sig,
            params: sig_snapshot.params,
            return_ty: sig_snapshot.return_ty,
            locals,
            body: body_stmts,
            has_this: true,
            this_class: Some(class_id),
            is_async,
            is_generator,
        });
        out.extend(mcls);
    }
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
            types: TyTable::default(),
            classes: vec![],
            enums: vec![],
            signatures: vec![Signature { params: vec![], return_ty: BackendTy::Void }],
            functions: vec![],
            globals: vec![], global_names: vec![],
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
