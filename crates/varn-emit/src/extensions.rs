use super::body::FnEmitter;
use super::free_functions::param_name;
use super::functions::fresh_sig;
use super::module_ctx::MCtx;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::{AstArena, AstId, Decl, Program, StmtId, StmtKind};
use varn_core::extension_names::ExtensionKind;
use varn_core::AtomInterner;
use varn_sem::output::TypeEntry;
use varn_tir::{BackendTy, DynReason, Signature, TirFunction, TyTable};

pub(super) fn collect_extension_names(
    program: &Program,
    ast_arena: &AstArena,
    out: &mut FxHashSet<Arc<str>>,
    interner: &AtomInterner,
    annotation_types: &FxHashMap<AstId, varn_sem::types::Type>,
    table: &varn_sem::types::CheckerTyTable,
) {
    use varn_core::ast::ExtensionMember;
    for &stmt in &program.body {
        let StmtKind::Decl(d) = &ast_arena.stmt(stmt).kind else {
            continue;
        };
        let Decl::Extension(ext) = d.as_ref() else {
            continue;
        };
        let Some(label) = extension_target_label(&ext.target, annotation_types, table, interner)
        else {
            continue;
        };
        for member in &ext.members {
            let name: Arc<str> = match member {
                ExtensionMember::Method(f) => {
                    Arc::from(ExtensionKind::Method.mangled(&label, interner.resolve(f.id)))
                }
                ExtensionMember::Getter { key, .. } => {
                    Arc::from(ExtensionKind::Getter.mangled(&label, interner.resolve(*key)))
                }
                ExtensionMember::Setter { key, .. } => {
                    Arc::from(ExtensionKind::Setter.mangled(&label, interner.resolve(*key)))
                }
            };
            out.insert(name);
        }
    }
}

pub(super) fn extension_target_label(
    target: &varn_core::ast::types::TypeNode,
    annotation_types: &FxHashMap<AstId, varn_sem::types::Type>,
    table: &varn_sem::types::CheckerTyTable,
    interner: &AtomInterner,
) -> Option<Arc<str>> {
    annotation_types
        .get(&target.id)?
        .extension_key(table, &|atom| {
            interner
                .try_resolve(atom)
                .or_else(|| table.name(atom))
                .map(Arc::from)
        })
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
        let Some(label) = extension_target_label(
            &ext.target,
            ctx.annotation_types,
            ctx.checker_table,
            ctx.interner,
        ) else {
            continue;
        };
        let recv_ty = ctx
            .annotation_types
            .get(&ext.target.id)
            .map(|target| {
                super::ty::lower_type(target, ctx.checker_table, ctx.interner, types, ctx.names)
            })
            .unwrap_or(BackendTy::Dynamic(DynReason::Unrecorded));
        let this_cid = match recv_ty {
            BackendTy::Class(cid) => Some(cid),
            BackendTy::Literal(_)
            | BackendTy::Int
            | BackendTy::Float
            | BackendTy::Bool
            | BackendTy::Char
            | BackendTy::Str
            | BackendTy::Bytes
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Dict(_)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Nullable(_)
            | BackendTy::Void
            | BackendTy::Never
            | BackendTy::Dynamic(_) => None,
        };
        for member in &ext.members {
            let (mangled, params, body): (Arc<str>, Vec<Arc<str>>, StmtId) = match member {
                ExtensionMember::Method(f) => (
                    Arc::from(ExtensionKind::Method.mangled(&label, ctx.interner.resolve(f.id))),
                    f.params
                        .iter()
                        .map(|p| param_name(p, ctx.interner))
                        .collect(),
                    f.body,
                ),
                ExtensionMember::Getter { key, body, .. } => (
                    Arc::from(ExtensionKind::Getter.mangled(&label, ctx.interner.resolve(*key))),
                    vec![],
                    *body,
                ),
                ExtensionMember::Setter {
                    key, param, body, ..
                } => (
                    Arc::from(ExtensionKind::Setter.mangled(&label, ctx.interner.resolve(*key))),
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
                } else if !matches!(recv_ty, BackendTy::Dynamic(_)) {
                    em = em.with_this_value(recv_ty);
                }
                let b = match &ast_arena.stmt(body).kind {
                    StmtKind::Block { stmts } => em.lower_block(stmts),
                    StmtKind::Empty
                    | StmtKind::Expr { .. }
                    | StmtKind::Decl(_)
                    | StmtKind::Error
                    | StmtKind::If { .. }
                    | StmtKind::While { .. }
                    | StmtKind::DoWhile { .. }
                    | StmtKind::For { .. }
                    | StmtKind::ForIn { .. }
                    | StmtKind::ForOf { .. }
                    | StmtKind::Return { .. }
                    | StmtKind::Break { .. }
                    | StmtKind::Continue { .. }
                    | StmtKind::Throw { .. }
                    | StmtKind::Try { .. }
                    | StmtKind::Using { .. }
                    | StmtKind::Labeled { .. }
                    | StmtKind::Debugger => em.lower_block(std::slice::from_ref(&body)),
                };
                (b, std::mem::take(&mut em.locals))
            };
            out.push(TirFunction {
                name: mangled,
                sig,
                params: vec![BackendTy::Dynamic(DynReason::Unrecorded); arity],
                return_ty: BackendTy::Dynamic(DynReason::Unrecorded),
                locals,
                body: body_stmts,
                has_this: true,
                this_class: this_cid,
                is_async: false,
                is_generator: false,
                has_rest: matches!(member, ExtensionMember::Method(f) if f.params.last().is_some_and(|p| p.is_rest)),
                force_inline: false,
            });
            out.extend(mcls);
        }
    }
}
