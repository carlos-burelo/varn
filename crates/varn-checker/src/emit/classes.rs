use super::class_members::emit_class_members;
use super::decl_classify::{anon_class_of, class_decl, enum_decl, namespace_decl};
use super::functions::{emit_member_fn, fresh_sig, lower_outer};
use super::module_ctx::MCtx;
use super::namespaces::ns_nested_types;
use super::ty::lower_type;
use super::ty::NameResolver;
use crate::binder::BindResult;
use crate::checker::TypeEntry;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::{AstArena, AstId, Param, Pattern, Program, StmtKind};
use varn_tir::{
    BackendTy, DynReason, Resolution, Signature, Span, TirExpr, TirExprKind, TirFunction, TirStmt,
    TyTable,
};

pub(super) fn param_field_assign(field: Arc<str>, param: u32) -> TirStmt {
    this_field_assign(
        field,
        TirExpr {
            kind: TirExprKind::Var,
            ty: BackendTy::Dynamic(DynReason::NotYetSupported),
            res: Resolution::Param(param),
            span: Span::EMPTY,
        },
    )
}

pub(super) fn this_field_assign(field: Arc<str>, value: TirExpr) -> TirStmt {
    let this = TirExpr {
        kind: TirExprKind::Var,
        ty: BackendTy::Dynamic(DynReason::NotYetSupported),
        res: Resolution::None,
        span: Span::EMPTY,
    };
    TirStmt::Expr(TirExpr {
        kind: TirExprKind::Assign {
            target: Box::new(TirExpr {
                kind: TirExprKind::Field {
                    object: Box::new(this),
                    name: field,
                },
                ty: BackendTy::Dynamic(DynReason::NotYetSupported),
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

pub(super) fn top_level_type_builds(program: &Program, ast_arena: &AstArena) -> u32 {
    let mut n = 0;
    for &stmt in &program.body {
        let StmtKind::Decl(d) = &ast_arena.stmt(stmt).kind else {
            continue;
        };
        if class_decl(d).is_some()
            || enum_decl(d).is_some()
            || anon_class_of(d, ast_arena).is_some()
        {
            n += 1;
        } else if let Some(ns) = namespace_decl(d) {
            n += ns_nested_types(ns).len() as u32;
        }
    }
    n
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_class_defs(
    program: &Program,
    ast_arena: &AstArena,
    bind: &BindResult,
    ctx: &MCtx,
    expr_table: &FxHashMap<AstId, crate::checker::TypeEntry>,
    types: &mut TyTable,
    signatures: &mut Vec<Signature>,
    functions: &mut Vec<TirFunction>,
    nested: &super::nested_types::NestedTypes,
    first_nested_ordinal: u32,
) -> Vec<varn_tir::TirClassDef> {
    use super::enums::emit_enum;
    let mut class_defs: Vec<varn_tir::TirClassDef> = Vec::new();
    let emit_type = |decl: &varn_core::ast::Decl,
                     class_defs: &mut Vec<varn_tir::TirClassDef>,
                     functions: &mut Vec<TirFunction>,
                     types: &mut TyTable,
                     signatures: &mut Vec<Signature>| {
        if let Some(class) = class_decl(decl).or_else(|| anon_class_of(decl, ast_arena)) {
            class_defs.push(emit_class(
                class, ast_arena, bind, ctx, expr_table, types, signatures, functions,
            ));
        } else if let Some(en) = enum_decl(decl) {
            class_defs.push(emit_enum(
                en, ast_arena, ctx, expr_table, types, signatures, functions,
            ));
        }
    };
    for &stmt in &program.body {
        let StmtKind::Decl(decl) = &ast_arena.stmt(stmt).kind else {
            continue;
        };
        if class_decl(decl).is_some()
            || enum_decl(decl).is_some()
            || anon_class_of(decl, ast_arena).is_some()
        {
            emit_type(decl, &mut class_defs, functions, types, signatures);
        } else if let Some(ns) = namespace_decl(decl) {
            for nested in ns_nested_types(ns) {
                emit_type(nested, &mut class_defs, functions, types, signatures);
            }
        }
    }
    debug_assert_eq!(class_defs.len() as u32, first_nested_ordinal);
    let mut emitted = 0;
    loop {
        let met = nested.met(ast_arena);
        if emitted == met.len() {
            break;
        }
        for decl in &met[emitted..] {
            emit_type(decl, &mut class_defs, functions, types, signatures);
        }
        emitted = met.len();
    }
    class_defs
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_class(
    class: &varn_core::ast::ClassDecl,
    ast_arena: &AstArena,
    bind: &BindResult,
    ctx: &MCtx,
    expr_table: &FxHashMap<AstId, TypeEntry>,
    types: &mut TyTable,
    signatures: &mut Vec<Signature>,
    out: &mut Vec<TirFunction>,
) -> varn_tir::TirClassDef {
    let class_name: Arc<str> = class
        .id
        .map(|a| Arc::from(ctx.interner.resolve(a)))
        .unwrap_or_else(|| Arc::from("<anon>"));
    let class_id = ctx.names.class_id(&class_name);

    let mut def = varn_tir::TirClassDef {
        name: class_name.clone(),
        class_id,
        ..Default::default()
    };

    if let Some(sup) = class.super_class {
        if let varn_core::ast::ExprKind::Identifier { name } = &ast_arena.expr(sup).kind {
            def.parent = ctx.names.class_id(ctx.interner.resolve(*name));
        }
        let (pre, x) = lower_outer(
            sup,
            ast_arena,
            ctx,
            expr_table,
            types,
            signatures,
            out,
            out.len() as u32,
            None,
        );
        def.prelude.extend(pre);
        def.super_class = Some(x);
    }
    for deco in &class.decorators {
        let (pre, x) = lower_outer(
            deco.expression,
            ast_arena,
            ctx,
            expr_table,
            types,
            signatures,
            out,
            out.len() as u32,
            class_id,
        );
        def.prelude.extend(pre);
        def.decorators.push(x);
    }

    let field_defaults: Vec<TirStmt> = {
        let mut stmts = Vec::new();
        let base = out.len() as u32;
        let mut cls: Vec<TirFunction> = Vec::new();
        let mut em = super::body::FnEmitter::new(
            ast_arena,
            expr_table,
            types,
            ctx.as_module_ctx(),
            signatures,
            &mut cls,
            base,
            vec![],
        );
        if let Some(cid) = class_id {
            em = em.with_this(cid);
        }
        for member in &class.body {
            if let varn_core::ast::ClassMember::Property {
                key,
                init: Some(init),
                modifiers,
                ..
            } = member
            {
                if modifiers.is_static {
                    continue;
                }
                let value = em.lower_expression(*init);
                stmts.append(&mut em.take_pending());
                stmts.push(this_field_assign(
                    Arc::from(ctx.interner.resolve(*key)),
                    value,
                ));
            }
        }
        drop(em);
        out.extend(cls);
        stmts
    };

    emit_class_members(
        class,
        &class_name,
        class_id,
        &mut def,
        ast_arena,
        bind,
        ctx,
        expr_table,
        types,
        signatures,
        out,
    );

    let primary: &[Param] = class.primary_params.as_deref().unwrap_or(&[]);
    let has_ctor = def.methods.iter().any(|m| m.key.as_ref() == "constructor");

    if !field_defaults.is_empty() || (!primary.is_empty() && !has_ctor) {
        match def.methods.iter().find(|m| m.key.as_ref() == "constructor") {
            Some(ctor) => {
                let body = &mut out[ctor.func.0 as usize].body;
                let mut new = field_defaults;
                new.extend(std::mem::take(body));
                *body = new;
            }
            None => {
                let sig = fresh_sig(signatures, primary.len());
                {
                    let tys: Vec<BackendTy> = primary
                        .iter()
                        .map(|p| {
                            p.type_ann
                                .as_ref()
                                .and_then(|t| ctx.annotation_types.get(&t.id))
                                .map(|resolved| {
                                    lower_type(
                                        resolved,
                                        ctx.checker_table,
                                        ctx.interner,
                                        types,
                                        ctx.names,
                                    )
                                })
                                .unwrap_or(BackendTy::Dynamic(DynReason::NotYetSupported))
                        })
                        .collect();
                    signatures[sig.0 as usize].params = tys;
                }
                let id = emit_member_fn(
                    Arc::from(format!("{class_name}.constructor")),
                    primary,
                    None,
                    ast_arena,
                    false,
                    false,
                    class_id,
                    None,
                    sig,
                    ctx,
                    expr_table,
                    types,
                    signatures,
                    out,
                );
                let body = &mut out[id.0 as usize].body;
                for (i, p) in primary.iter().enumerate() {
                    if p.modifiers.visibility.is_some() || p.modifiers.is_readonly {
                        continue;
                    }
                    if let Pattern::Identifier { name, .. } = &p.pattern {
                        body.push(param_field_assign(
                            Arc::from(ctx.interner.resolve(*name)),
                            i as u32,
                        ));
                    }
                }
                let mut new = field_defaults;
                new.extend(std::mem::take(body));
                *body = new;
                def.methods.push(varn_tir::TirClassMember {
                    key: Arc::from("constructor"),
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
