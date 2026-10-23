use super::functions::{emit_member_fn, fresh_sig, lower_outer};
use super::module_ctx::MCtx;
use super::ty::NameResolver;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::{AstArena, AstId};
use varn_sem::output::TypeEntry;
use varn_tir::{Signature, TirFunction, TyTable};
pub(super) fn emit_enum(
    en: &varn_core::ast::EnumDecl,
    ast_arena: &AstArena,
    ctx: &MCtx,
    expr_table: &FxHashMap<AstId, TypeEntry>,
    types: &mut TyTable,
    signatures: &mut Vec<Signature>,
    out: &mut Vec<TirFunction>,
) -> varn_tir::TirClassDef {
    use varn_core::ast::ClassMember;
    let name: Arc<str> = crate::namespace_scope::declared_name(
        ctx.type_names,
        en.ast_id,
        ctx.interner.resolve(en.id),
    );
    let enum_id = ctx.names.enum_id(&name);
    let mut def = varn_tir::TirClassDef {
        name: name.clone(),
        enum_id,
        ..Default::default()
    };

    let shared_fields: Vec<&str> = en
        .body
        .iter()
        .filter_map(|member| match member {
            ClassMember::Property { key, modifiers, .. } if !modifiers.is_static => {
                Some(ctx.interner.resolve(*key))
            }
            ClassMember::Property { .. }
            | ClassMember::Constructor { .. }
            | ClassMember::Destructor { .. }
            | ClassMember::Method { .. }
            | ClassMember::Getter { .. }
            | ClassMember::Setter { .. }
            | ClassMember::StaticBlock { .. } => None,
        })
        .collect();
    let mut tag = 0i64;
    for m in &en.members {
        if let Some(init) = m.init
            && let varn_core::ast::ExprKind::IntLiteral { value, .. } = &ast_arena.expr(init).kind
        {
            tag = *value;
        }
        let member_name = ctx.interner.resolve(m.id);
        let fields: Vec<Arc<str>> = m
            .payload_fields
            .iter()
            .map(|f| ctx.interner.resolve(f.name))
            .chain(shared_fields.iter().copied())
            .map(Arc::from)
            .collect();
        let mut const_args = Vec::new();
        for f in &m.payload_fields {
            if let Some(init) = f.init {
                let (pre, x) = lower_outer(
                    init,
                    ast_arena,
                    ctx,
                    expr_table,
                    types,
                    signatures,
                    out,
                    out.len() as u32,
                    None,
                    name.rsplit_once("::").map(|(outer, _)| outer),
                );
                def.prelude.extend(pre);
                const_args.push(x);
            }
        }
        def.variants.push(varn_tir::TirVariantDef {
            name: Arc::from(member_name),
            tag,
            fields,
            const_args,
        });
        tag += 1;
    }

    let this_cid = ctx.names.class_id(&name);
    for member in &en.body {
        match member {
            ClassMember::Method {
                key,
                params,
                body: Some(body),
                modifiers,
                decorators,
                ..
            } => {
                let key_str = ctx.interner.resolve(*key);
                let sig = enum_member_sig(ctx, enum_id, key_str, params.len(), signatures);
                let id = emit_member_fn(
                    Arc::from(format!("{name}.{key_str}")),
                    params,
                    Some(*body),
                    ast_arena,
                    modifiers.is_async,
                    modifiers.is_generator,
                    (!modifiers.is_static).then_some(()).and(this_cid),
                    (!modifiers.is_static).then_some(()).and(enum_id),
                    sig,
                    ctx,
                    expr_table,
                    types,
                    signatures,
                    out,
                );
                def.methods.push(varn_tir::TirClassMember {
                    key: Arc::from(key_str),
                    func: id,
                    is_static: modifiers.is_static,
                    is_private: false,
                    decorators: super::decorators::lower_decorator_exprs(
                        decorators,
                        ast_arena,
                        ctx,
                        expr_table,
                        types,
                        signatures,
                        out,
                        &mut def.prelude,
                        None,
                    ),
                });
            }
            ClassMember::Constructor {
                params,
                body,
                decorators,
                ..
            } => {
                let sig = fresh_sig(signatures, params.len());
                let id = emit_member_fn(
                    Arc::from(format!("{name}.constructor")),
                    params,
                    Some(*body),
                    ast_arena,
                    false,
                    false,
                    this_cid,
                    enum_id,
                    sig,
                    ctx,
                    expr_table,
                    types,
                    signatures,
                    out,
                );
                def.methods.push(varn_tir::TirClassMember {
                    key: Arc::from(varn_core::TokenKind::Constructor.as_str()),
                    func: id,
                    is_static: false,
                    is_private: false,
                    decorators: super::decorators::lower_decorator_exprs(
                        decorators,
                        ast_arena,
                        ctx,
                        expr_table,
                        types,
                        signatures,
                        out,
                        &mut def.prelude,
                        None,
                    ),
                });
            }
            ClassMember::Getter {
                key,
                body: Some(body),
                modifiers,
                decorators,
                ..
            } => {
                let key_str = ctx.interner.resolve(*key);
                let accessor = varn_core::AccessorKind::Get.key(key_str);
                let sig = enum_member_sig(ctx, enum_id, &accessor, 0, signatures);
                let id = emit_member_fn(
                    Arc::from(format!("{name}.{accessor}")),
                    &[],
                    Some(*body),
                    ast_arena,
                    false,
                    false,
                    (!modifiers.is_static).then_some(()).and(this_cid),
                    (!modifiers.is_static).then_some(()).and(enum_id),
                    sig,
                    ctx,
                    expr_table,
                    types,
                    signatures,
                    out,
                );
                def.accessors.push(varn_tir::TirClassAccessor {
                    key: Arc::from(key_str),
                    func: id,
                    is_getter: true,
                    is_static: modifiers.is_static,
                    decorators: super::decorators::lower_decorator_exprs(
                        decorators,
                        ast_arena,
                        ctx,
                        expr_table,
                        types,
                        signatures,
                        out,
                        &mut def.prelude,
                        None,
                    ),
                });
            }
            ClassMember::Setter {
                key,
                param,
                body: Some(body),
                modifiers,
                decorators,
                ..
            } => {
                let key_str = ctx.interner.resolve(*key);
                let accessor = varn_core::AccessorKind::Set.key(key_str);
                let sig = enum_member_sig(ctx, enum_id, &accessor, 1, signatures);
                let id = emit_member_fn(
                    Arc::from(format!("{name}.{accessor}")),
                    std::slice::from_ref(param),
                    Some(*body),
                    ast_arena,
                    false,
                    false,
                    (!modifiers.is_static).then_some(()).and(this_cid),
                    (!modifiers.is_static).then_some(()).and(enum_id),
                    sig,
                    ctx,
                    expr_table,
                    types,
                    signatures,
                    out,
                );
                def.accessors.push(varn_tir::TirClassAccessor {
                    key: Arc::from(key_str),
                    func: id,
                    is_getter: false,
                    is_static: modifiers.is_static,
                    decorators: super::decorators::lower_decorator_exprs(
                        decorators,
                        ast_arena,
                        ctx,
                        expr_table,
                        types,
                        signatures,
                        out,
                        &mut def.prelude,
                        None,
                    ),
                });
            }
            ClassMember::Property {
                key,
                init,
                modifiers,
                decorators,
                ..
            } => {
                if !decorators.is_empty() {
                    let decos = super::decorators::lower_decorator_exprs(
                        decorators,
                        ast_arena,
                        ctx,
                        expr_table,
                        types,
                        signatures,
                        out,
                        &mut def.prelude,
                        None,
                    );
                    if !decos.is_empty() {
                        def.property_decorators
                            .push(varn_tir::TirPropertyDecorator {
                                key: Arc::from(ctx.interner.resolve(*key)),
                                is_static: modifiers.is_static,
                                decorators: decos,
                            });
                    }
                }
                if modifiers.is_static {
                    let init_x = init.map(|e| {
                        let (pre, x) = lower_outer(
                            e,
                            ast_arena,
                            ctx,
                            expr_table,
                            types,
                            signatures,
                            out,
                            out.len() as u32,
                            None,
                            name.rsplit_once("::").map(|(outer, _)| outer),
                        );
                        def.prelude.extend(pre);
                        x
                    });
                    def.statics
                        .push((Arc::from(ctx.interner.resolve(*key)), init_x));
                }
            }
            ClassMember::StaticBlock { body, .. } => {
                let sig = fresh_sig(signatures, 0);
                let id = emit_member_fn(
                    Arc::from(format!("{name}.<static>")),
                    &[],
                    Some(*body),
                    ast_arena,
                    false,
                    false,
                    None,
                    None,
                    sig,
                    ctx,
                    expr_table,
                    types,
                    signatures,
                    out,
                );
                def.static_blocks.push(id);
            }
            ClassMember::Destructor { .. }
            | ClassMember::Method { .. }
            | ClassMember::Getter { .. }
            | ClassMember::Setter { .. } => {}
        }
    }
    def
}

fn enum_member_sig(
    ctx: &MCtx,
    enum_id: Option<varn_tir::EnumId>,
    key: &str,
    arity: usize,
    signatures: &mut Vec<varn_tir::Signature>,
) -> varn_tir::SigId {
    enum_id
        .and_then(|id| ctx.enums.get(id.0 as usize))
        .and_then(|info| {
            let slot = info.member_slot(key)?;
            Some(info.member_at(slot)?.sig)
        })
        .unwrap_or_else(|| fresh_sig(signatures, arity))
}
