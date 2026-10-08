use super::decorators::lower_decorator_exprs;
use super::functions::{emit_member_fn, fresh_sig, lower_outer};
use super::module_ctx::MCtx;
use crate::binder::BindResult;
use crate::checker::TypeEntry;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::{AstArena, AstId};
use varn_tir::{SigId, Signature, TirFunction, TyTable};

pub(super) fn class_info_sig(
    ctx: &MCtx,
    class_id: Option<varn_tir::ClassId>,
    key: &str,
    arity: usize,
    signatures: &mut Vec<Signature>,
) -> SigId {
    class_id
        .and_then(|cid| {
            let info = &ctx.classes[cid.0 as usize];
            info.method_slot(key)
                .and_then(|s| info.method_at(s))
                .map(|e| e.sig)
        })
        .unwrap_or_else(|| fresh_sig(signatures, arity))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_class_members(
    class: &varn_core::ast::ClassDecl,
    class_name: &Arc<str>,
    class_id: Option<varn_tir::ClassId>,
    def: &mut varn_tir::TirClassDef,
    ast_arena: &AstArena,
    bind: &BindResult,
    ctx: &MCtx,
    expr_table: &FxHashMap<AstId, TypeEntry>,
    types: &mut TyTable,
    signatures: &mut Vec<Signature>,
    out: &mut Vec<TirFunction>,
) {
    use varn_core::ast::ClassMember;
    for member in &class.body {
        match member {
            ClassMember::Constructor {
                params,
                body,
                decorators,
                ..
            } => {
                let sig = class_id
                    .and_then(|cid| ctx.classes[cid.0 as usize].constructor)
                    .unwrap_or_else(|| fresh_sig(signatures, params.len()));
                let id = emit_member_fn(
                    Arc::from(format!("{class_name}.constructor")),
                    params,
                    Some(*body),
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
                def.methods.push(varn_tir::TirClassMember {
                    key: Arc::from("constructor"),
                    func: id,
                    is_static: false,
                    is_private: false,
                    decorators: lower_decorator_exprs(
                        decorators,
                        ast_arena,
                        ctx,
                        expr_table,
                        types,
                        signatures,
                        out,
                        &mut def.prelude,
                        class_id,
                    ),
                });
            }
            ClassMember::Method {
                key,
                params,
                body: Some(body),
                modifiers,
                decorators,
                ..
            } => {
                let key_str = ctx.interner.resolve(*key);
                let sig = if modifiers.is_static {
                    super::functions::static_method_sig(
                        bind, class_name, key_str, ctx, types, signatures,
                    )
                    .unwrap_or_else(|| {
                        class_info_sig(ctx, class_id, key_str, params.len(), signatures)
                    })
                } else {
                    class_info_sig(ctx, class_id, key_str, params.len(), signatures)
                };
                let id = emit_member_fn(
                    Arc::from(format!("{class_name}.{key_str}")),
                    params,
                    Some(*body),
                    ast_arena,
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
                let decos = lower_decorator_exprs(
                    decorators,
                    ast_arena,
                    ctx,
                    expr_table,
                    types,
                    signatures,
                    out,
                    &mut def.prelude,
                    class_id,
                );
                def.methods.push(varn_tir::TirClassMember {
                    key: Arc::from(key_str),
                    func: id,
                    is_static: modifiers.is_static,
                    is_private: matches!(
                        modifiers.visibility,
                        Some(varn_core::ast::operators::Visibility::Private)
                    ),
                    decorators: decos,
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
                let sig = fresh_sig(signatures, 0);
                let id = emit_member_fn(
                    Arc::from(format!("{class_name}.get {key_str}")),
                    &[],
                    Some(*body),
                    ast_arena,
                    false,
                    false,
                    (!modifiers.is_static).then_some(()).and(class_id),
                    None,
                    sig,
                    ctx,
                    expr_table,
                    types,
                    signatures,
                    out,
                );
                let decos = lower_decorator_exprs(
                    decorators,
                    ast_arena,
                    ctx,
                    expr_table,
                    types,
                    signatures,
                    out,
                    &mut def.prelude,
                    class_id,
                );
                def.accessors.push(varn_tir::TirClassAccessor {
                    key: Arc::from(key_str),
                    func: id,
                    is_getter: true,
                    is_static: modifiers.is_static,
                    decorators: decos,
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
                let sig = fresh_sig(signatures, 1);
                let ps = std::slice::from_ref(param);
                let id = emit_member_fn(
                    Arc::from(format!("{class_name}.set {key_str}")),
                    ps,
                    Some(*body),
                    ast_arena,
                    false,
                    false,
                    (!modifiers.is_static).then_some(()).and(class_id),
                    None,
                    sig,
                    ctx,
                    expr_table,
                    types,
                    signatures,
                    out,
                );
                let decos = lower_decorator_exprs(
                    decorators,
                    ast_arena,
                    ctx,
                    expr_table,
                    types,
                    signatures,
                    out,
                    &mut def.prelude,
                    class_id,
                );
                def.accessors.push(varn_tir::TirClassAccessor {
                    key: Arc::from(key_str),
                    func: id,
                    is_getter: false,
                    is_static: modifiers.is_static,
                    decorators: decos,
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
                    let decos = lower_decorator_exprs(
                        decorators,
                        ast_arena,
                        ctx,
                        expr_table,
                        types,
                        signatures,
                        out,
                        &mut def.prelude,
                        class_id,
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
                            class_id,
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
                    Arc::from(format!("{class_name}.<static>")),
                    &[],
                    Some(*body),
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
                def.static_blocks.push(id);
            }
            ClassMember::Destructor { .. }
            | ClassMember::Method { .. }
            | ClassMember::Getter { .. }
            | ClassMember::Setter { .. } => {}
        }
    }
}
