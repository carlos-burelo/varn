use super::body::FnEmitter;
use super::free_functions::param_name;
use super::functions::fresh_sig;
use super::module_ctx::MCtx;
use super::ty::NameResolver;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::{AstArena, AstId, Decl, Program, StmtId, StmtKind};
use varn_core::AtomInterner;
use varn_sem::output::TypeEntry;
use varn_tir::{BackendTy, DynReason, Signature, TirFunction, TyTable};

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
        TypeKind::This
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Object(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => None,
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
                .unwrap_or(BackendTy::Dynamic(DynReason::NotYetSupported)),
        };
        let this_cid = match recv_ty {
            BackendTy::Class(cid) => Some(cid),
            BackendTy::Int
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
                    | StmtKind::Switch { .. }
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
                params: vec![BackendTy::Dynamic(DynReason::NotYetSupported); arity],
                return_ty: BackendTy::Dynamic(DynReason::NotYetSupported),
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
