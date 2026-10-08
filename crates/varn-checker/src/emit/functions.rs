use super::body::FnEmitter;
use super::module_ctx::MCtx;
use super::tables;
use super::ty::lower_type;
use crate::binder::BindResult;
use crate::checker::TypeEntry;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::{
    AstArena, AstId, ExprId, FunctionDecl, Param, Pattern, Program, StmtId, StmtKind,
};
use varn_core::{Atom, AtomInterner, TypeKind};
use varn_tir::{BackendTy, DynReason, SigId, Signature, TirExpr, TirFunction, TirStmt, TyTable};

pub(super) fn param_name(p: &Param, interner: &AtomInterner) -> Arc<str> {
    match &p.pattern {
        Pattern::Identifier { name, .. } => Arc::from(interner.resolve(*name)),
        Pattern::Array { .. }
        | Pattern::Object { .. }
        | Pattern::Assignment { .. }
        | Pattern::Rest { .. } => Arc::from("_"),
    }
}

pub(super) struct FreeFunctions<'a> {
    pub(super) list: Vec<(&'a FunctionDecl, Option<Arc<str>>)>,
    pub(super) index: FxHashMap<Atom, (u32, u32)>,
    pub(super) decorated: FxHashSet<Atom>,
}

pub(super) fn collect_free_functions<'a>(
    program: &Program,
    ast_arena: &'a AstArena,
    interner: &'a AtomInterner,
    shadowed: &FxHashSet<u32>,
) -> FreeFunctions<'a> {
    use super::decl_classify::{free_function, namespace_decl};
    use varn_core::ast::NamespaceDecl;
    fn ns_member_fns<'a>(
        ns: &'a NamespaceDecl,
        interner: &AtomInterner,
        out: &mut Vec<(&'a FunctionDecl, Option<Arc<str>>)>,
    ) {
        let ns_name: Arc<str> = Arc::from(interner.resolve(ns.id));
        for m in &ns.body {
            let inner = match m {
                varn_core::ast::Decl::Export(varn_core::ast::ExportDecl::Decl {
                    declaration,
                    ..
                }) => declaration.as_ref(),
                other @ varn_core::ast::Decl::Variable(_)
                | other @ varn_core::ast::Decl::Function(_)
                | other @ varn_core::ast::Decl::Class(_)
                | other @ varn_core::ast::Decl::Interface(_)
                | other @ varn_core::ast::Decl::TypeAlias(_)
                | other @ varn_core::ast::Decl::Enum(_)
                | other @ varn_core::ast::Decl::Namespace(_)
                | other @ varn_core::ast::Decl::Import(_)
                | other @ varn_core::ast::Decl::Export(_)
                | other @ varn_core::ast::Decl::Extension(_)
                | other @ varn_core::ast::Decl::Struct(_)
                | other @ varn_core::ast::Decl::SumType(_) => other,
            };
            match inner {
                varn_core::ast::Decl::Function(f) => out.push((f, Some(ns_name.clone()))),
                varn_core::ast::Decl::Namespace(inner_ns) => ns_member_fns(inner_ns, interner, out),
                varn_core::ast::Decl::Variable(_)
                | varn_core::ast::Decl::Class(_)
                | varn_core::ast::Decl::Interface(_)
                | varn_core::ast::Decl::TypeAlias(_)
                | varn_core::ast::Decl::Enum(_)
                | varn_core::ast::Decl::Import(_)
                | varn_core::ast::Decl::Export(_)
                | varn_core::ast::Decl::Extension(_)
                | varn_core::ast::Decl::Struct(_)
                | varn_core::ast::Decl::SumType(_) => {}
            }
        }
    }
    let mut free_fns: Vec<(&FunctionDecl, Option<Arc<str>>)> = program
        .body
        .iter()
        .filter_map(|&s| match &ast_arena.stmt(s).kind {
            StmtKind::Decl(d) => free_function(d).map(|f| (f, None)),
            StmtKind::Block { .. }
            | StmtKind::Empty
            | StmtKind::Expr { .. }
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
            | StmtKind::Debugger => None,
        })
        .collect();
    for &stmt in &program.body {
        let StmtKind::Decl(d) = &ast_arena.stmt(stmt).kind else {
            continue;
        };
        if let Some(ns) = namespace_decl(d) {
            ns_member_fns(ns, interner, &mut free_fns);
        }
    }
    let mut fn_index: FxHashMap<Atom, (u32, u32)> = FxHashMap::default();
    for (i, (f, _)) in free_fns.iter().enumerate() {
        fn_index
            .entry(f.id)
            .or_insert((i as u32, f.params.len() as u32));
    }
    let decorated_fns: FxHashSet<Atom> = free_fns
        .iter()
        .filter(|(f, ns)| {
            ns.is_none()
                && f.decorators.iter().any(|d| {
                    !varn_core::ast::decorators::is_active_builtin(
                        ast_arena,
                        interner,
                        |off| shadowed.contains(&off),
                        d,
                    )
                })
        })
        .map(|(f, _)| f.id)
        .collect();
    FreeFunctions {
        list: free_fns,
        index: fn_index,
        decorated: decorated_fns,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_member_fn(
    display_name: Arc<str>,
    params: &[Param],
    body: Option<StmtId>,
    ast_arena: &AstArena,
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
    let param_names: Vec<Arc<str>> = params.iter().map(|p| param_name(p, ctx.interner)).collect();
    let fn_id = varn_tir::FnId(out.len() as u32);
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
            param_names,
        );
        if let Some(cid) = this_class {
            em = em.with_this(cid);
        }
        if let Some(eid) = this_enum {
            em = em.with_this_enum(eid);
        }
        let mut b = em.destructure_params(params);
        for (i, p) in params.iter().enumerate() {
            if p.modifiers.visibility.is_some() || p.modifiers.is_readonly {
                if let Pattern::Identifier { name, .. } = &p.pattern {
                    b.push(em.this_param_field_assign(
                        Arc::from(ctx.interner.resolve(*name)),
                        i as u32,
                        sig_snapshot.params[i],
                    ));
                }
            }
        }
        b.extend(match body {
            Some(id) => match &ast_arena.stmt(id).kind {
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
                | StmtKind::Debugger => em.lower_block(std::slice::from_ref(&id)),
            },
            None => em.lower_block(&[]),
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
        has_rest: params.last().is_some_and(|p| p.is_rest),
        force_inline: false,
    });
    out.extend(mcls);
    fn_id
}

pub(super) fn fresh_sig(signatures: &mut Vec<Signature>, arity: usize) -> SigId {
    let id = SigId(signatures.len() as u32);
    signatures.push(Signature {
        params: vec![BackendTy::Dynamic(DynReason::NotYetSupported); arity],
        return_ty: BackendTy::Dynamic(DynReason::NotYetSupported),
        has_rest: false,
    });
    id
}

pub(super) fn lower_outer(
    e: ExprId,
    ast_arena: &AstArena,
    ctx: &MCtx,
    expr_table: &FxHashMap<AstId, TypeEntry>,
    types: &mut TyTable,
    signatures: &mut Vec<Signature>,
    closures: &mut Vec<TirFunction>,
    base: u32,
    this_class: Option<varn_tir::ClassId>,
) -> (Vec<TirStmt>, TirExpr) {
    let mut em = FnEmitter::new(
        ast_arena,
        expr_table,
        types,
        ctx.as_module_ctx(),
        signatures,
        closures,
        base,
        vec![],
    );
    if let Some(cid) = this_class {
        em = em.with_this(cid);
    }
    em.lower_outer_expr(e)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn static_method_sig(
    bind: &BindResult,
    class_name: &str,
    key: &str,
    ctx: &MCtx,
    types: &mut TyTable,
    signatures: &mut Vec<Signature>,
) -> Option<SigId> {
    let members = &bind.type_members.classes.get(class_name)?.members;
    let ty = members
        .iter()
        .filter(|m| {
            m.is_static
                && m.name.as_ref() == key
                && matches!(
                    m.kind,
                    crate::types::ClassMemberKind::Method | crate::types::ClassMemberKind::Function
                )
        })
        .find_map(|m| matches!(m.ty.kind(ctx.checker_table), TypeKind::Fn(_)).then(|| m.ty))?;
    Some(tables::intern_signature(
        &ty,
        ctx.checker_table,
        ctx.interner,
        types,
        ctx.names,
        signatures,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_function(
    f: &FunctionDecl,
    ns: Option<&str>,
    ast_arena: &AstArena,
    bind: &BindResult,
    expr_table: &FxHashMap<AstId, TypeEntry>,
    types: &mut TyTable,
    ctx: &MCtx,
    signatures: &mut Vec<Signature>,
    closures: &mut Vec<TirFunction>,
    closure_base: u32,
) -> TirFunction {
    let fn_name_str = ctx.interner.resolve(f.id);
    let sym_ty = bind
        .global_symbols()
        .find(|s| s.name == f.id)
        .and_then(|s| s.ty);
    let fn_ty: Option<crate::types::Type> = match ns {
        Some(ns) => bind.get_namespace_members_local(ns).and_then(|members| {
            members
                .iter()
                .filter(|m| {
                    m.kind == crate::types::ClassMemberKind::Function
                        && m.name.as_ref() == fn_name_str
                })
                .find_map(|m| matches!(m.ty.kind(ctx.checker_table), TypeKind::Fn(_)).then(|| m.ty))
        }),
        None => sym_ty.filter(|t| matches!(t.kind(ctx.checker_table), TypeKind::Fn(_))),
    };

    let arity = f.params.len();
    let mut param_tys = vec![BackendTy::Dynamic(DynReason::NotYetSupported); arity];
    let mut return_ty = BackendTy::Dynamic(DynReason::NotYetSupported);
    if let Some(TypeKind::Fn(fn_id)) = fn_ty.as_ref().map(|t| t.kind(ctx.checker_table)) {
        let ft = ctx.checker_table.get_function(fn_id);
        for (i, p) in ft.params.iter().take(arity).enumerate() {
            let inner = lower_type(
                &crate::types::Type::resolved(p.ty),
                ctx.checker_table,
                ctx.interner,
                types,
                ctx.names,
            );
            param_tys[i] = match inner {
                BackendTy::Dynamic(_) | BackendTy::Nullable(_) => inner,
                _ if p.optional => BackendTy::Nullable(types.intern(inner)),
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
                | BackendTy::Class(_)
                | BackendTy::Enum(_)
                | BackendTy::Fn(_)
                | BackendTy::Void
                | BackendTy::Never => inner,
            };
        }
        return_ty = if ns.is_some() && f.return_type.is_none() {
            BackendTy::Dynamic(DynReason::NotYetSupported)
        } else {
            lower_type(
                &crate::types::Type::resolved(ft.return_type),
                ctx.checker_table,
                ctx.interner,
                types,
                ctx.names,
            )
        };
    }

    let sig = SigId(signatures.len() as u32);
    signatures.push(Signature {
        params: param_tys.clone(),
        return_ty,
        has_rest: f.params.last().is_some_and(|p| p.is_rest),
    });

    let param_names: Vec<Arc<str>> = f
        .params
        .iter()
        .map(|p| param_name(p, ctx.interner))
        .collect();
    let (body, locals) = {
        let mut em = FnEmitter::new(
            ast_arena,
            expr_table,
            types,
            ctx.as_module_ctx(),
            signatures,
            closures,
            closure_base,
            param_names,
        );
        let mut b = em.destructure_params(&f.params);
        b.extend(match &ast_arena.stmt(f.body).kind {
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
            | StmtKind::Debugger => em.lower_block(std::slice::from_ref(&f.body)),
        });
        (b, std::mem::take(&mut em.locals))
    };

    TirFunction {
        name: Arc::from(fn_name_str),
        sig,
        params: param_tys,
        return_ty,
        locals,
        body,
        has_this: false,
        this_class: None,
        is_async: f.modifiers.is_async,
        is_generator: f.modifiers.is_generator,
        has_rest: f.params.last().is_some_and(|p| p.is_rest),
        force_inline: varn_core::ast::decorators::match_builtin(
            ast_arena,
            ctx.interner,
            &f.decorators,
        )
        .into_iter()
        .zip(f.decorators.iter())
        .any(|(m, d)| {
            !bind.user_decorators.contains(&d.range.start.offset)
                && matches!(
                    m.result,
                    Some(Ok(varn_core::ast::decorators::BuiltinDecorator::Inline))
                )
        }),
    }
}
