use super::body::FnEmitter;
use super::free_functions::param_name;
use super::functions::fresh_sig;
use super::module_ctx::MCtx;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use vn_core::ast::{AstArena, AstId, Decl, Program, StmtId, StmtKind};
use vn_core::extension_names::ExtensionKind;
use vn_core::AtomInterner;
use vn_sem::output::TypeEntry;
use vn_tir::{BackendTy, DynReason, Signature, TirFunction, TyTable};

pub(super) fn top_level_extensions<'a>(
    program: &'a Program,
    ast_arena: &'a AstArena,
) -> Vec<(&'a vn_core::ast::ExtensionDecl, bool)> {
    program
        .body
        .iter()
        .filter_map(|&stmt| {
            let StmtKind::Decl(d) = &ast_arena.stmt(stmt).kind else {
                return None;
            };
            match d.as_ref() {
                Decl::Extension(ext) => Some((ext, false)),
                Decl::Export(vn_core::ast::ExportDecl::Decl { declaration, .. }) => {
                    match declaration.as_ref() {
                        Decl::Extension(ext) => Some((ext, true)),
                        Decl::Variable(_)
                        | Decl::Function(_)
                        | Decl::Class(_)
                        | Decl::Interface(_)
                        | Decl::TypeAlias(_)
                        | Decl::Enum(_)
                        | Decl::Namespace(_)
                        | Decl::Import(_)
                        | Decl::Export(_)
                        | Decl::SumType(_) => None,
                    }
                }
                Decl::Variable(_)
                | Decl::Function(_)
                | Decl::Class(_)
                | Decl::Interface(_)
                | Decl::TypeAlias(_)
                | Decl::Enum(_)
                | Decl::Namespace(_)
                | Decl::Import(_)
                | Decl::Export(_)
                | Decl::SumType(_) => None,
            }
        })
        .collect()
}

pub(super) fn collect_extension_names(
    program: &Program,
    ast_arena: &AstArena,
    out: &mut FxHashSet<Arc<str>>,
    interner: &AtomInterner,
    annotation_types: &FxHashMap<AstId, vn_sem::types::Type>,
    table: &vn_sem::types::CheckerTyTable,
) {
    use vn_core::ast::ExtensionMember;
    for (ext, _) in top_level_extensions(program, ast_arena) {
        let Some(label) = extension_target_label(&ext.target, annotation_types, table, interner)
        else {
            continue;
        };
        for member in &ext.members {
            let name: Arc<str> = match member {
                ExtensionMember::Method(f) if f.modifiers.is_declare => continue,
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
    target: &vn_core::ast::types::TypeNode,
    annotation_types: &FxHashMap<AstId, vn_sem::types::Type>,
    table: &vn_sem::types::CheckerTyTable,
    interner: &AtomInterner,
) -> Option<Arc<str>> {
    vn_sem::bind::extension_label(target.id, annotation_types, table, interner)
}

pub(super) fn exported_extension_names(
    program: &Program,
    ast_arena: &AstArena,
    bind: &vn_sem::bind::BindResult,
) -> Vec<Arc<str>> {
    let mut out = Vec::new();
    for (ext, exported) in top_level_extensions(program, ast_arena) {
        if !exported {
            continue;
        }
        let Some(label) = extension_target_label(
            &ext.target,
            &bind.annotation_types,
            &bind.ty_table,
            &bind.interner,
        ) else {
            continue;
        };
        out.extend(
            vn_sem::bind::extension_names(ext, &label, &bind.interner)
                .into_iter()
                .map(|name| name.mangled)
                .filter(|mangled| !bind.extensions.forwarded.contains(mangled)),
        );
    }
    out
}

pub(super) fn extension_imports(
    bind: &vn_sem::bind::BindResult,
    desugar: &vn_sem::output::Desugarings,
    declared: &FxHashSet<Arc<str>>,
) -> Vec<vn_tir::TirImport> {
    let mut used: Vec<Arc<str>> = desugar
        .extension_calls
        .values()
        .chain(desugar.extension_members.values())
        .chain(desugar.extension_set_members.values())
        .filter(|name| !declared.contains(name.as_ref()))
        .cloned()
        .collect();
    used.sort();
    used.dedup();
    let mut by_origin: std::collections::BTreeMap<Arc<str>, Vec<Arc<str>>> =
        std::collections::BTreeMap::new();
    for name in used {
        let Some(origin) = bind
            .interner
            .get(name.as_ref())
            .and_then(|atom| bind.scopes.get(bind.global_scope).bindings.get(&atom))
            .and_then(|&id| bind.arena.get(id).origin_module)
            .and_then(|atom| {
                bind.interner
                    .try_resolve(atom)
                    .or_else(|| bind.ty_table.name(atom))
            })
        else {
            continue;
        };
        by_origin.entry(Arc::from(origin)).or_default().push(name);
    }
    by_origin
        .into_iter()
        .map(|(source, names)| vn_tir::TirImport {
            source,
            is_type_only: false,
            specs: names
                .into_iter()
                .map(|name| vn_tir::TirImportSpec {
                    local: name.clone(),
                    kind: vn_tir::TirImportKind::Named(name),
                    slot: None,
                })
                .collect(),
        })
        .collect()
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
    use vn_core::ast::ExtensionMember;
    for (ext, _) in top_level_extensions(program, ast_arena) {
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
            if matches!(member, ExtensionMember::Method(f) if f.modifiers.is_declare) {
                continue;
            }
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
