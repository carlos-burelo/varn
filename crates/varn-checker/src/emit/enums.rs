use super::functions::{emit_member_fn, fresh_sig, lower_outer};
use super::module_ctx::MCtx;
use super::ty::NameResolver;
use crate::checker::TypeEntry;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::{AstArena, AstId};
use varn_tir::{Signature, TirFunction, TyTable};

#[allow(clippy::too_many_arguments)]
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
    let name: Arc<str> = Arc::from(ctx.interner.resolve(en.id));
    let enum_id = ctx.names.enum_id(&name);
    let mut def = varn_tir::TirClassDef {
        name: name.clone(),
        enum_id,
        ..Default::default()
    };

    let mut tag = 0i64;
    for m in &en.members {
        if let Some(init) = m.init {
            if let varn_core::ast::ExprKind::IntLiteral { value, .. } = &ast_arena.expr(init).kind {
                tag = *value;
            }
        }
        let member_name = ctx.interner.resolve(m.id);
        let fields_str = m
            .payload_fields
            .iter()
            .map(|f| ctx.interner.resolve(f.name))
            .collect::<Vec<&str>>()
            .join(",");
        let meta = if fields_str.is_empty() {
            format!("{name}.{member_name}")
        } else {
            format!("{name}.{member_name}:{fields_str}")
        };
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
                );
                def.prelude.extend(pre);
                const_args.push(x);
            }
        }
        def.variants.push(varn_tir::TirVariantDef {
            name: Arc::from(member_name),
            tag,
            meta: Arc::from(meta.as_str()),
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
                ..
            } => {
                let key_str = ctx.interner.resolve(*key);
                let sig = fresh_sig(signatures, params.len());
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
                    decorators: vec![],
                });
            }
            ClassMember::Constructor { params, body, .. } => {
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
                    key: Arc::from("constructor"),
                    func: id,
                    is_static: false,
                    is_private: false,
                    decorators: vec![],
                });
            }
            ClassMember::Getter {
                key,
                body: Some(body),
                modifiers,
                ..
            } => {
                let key_str = ctx.interner.resolve(*key);
                let sig = fresh_sig(signatures, 0);
                let id = emit_member_fn(
                    Arc::from(format!("{name}.get {key_str}")),
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
                });
            }
            ClassMember::Setter {
                key,
                param,
                body: Some(body),
                modifiers,
                ..
            } => {
                let key_str = ctx.interner.resolve(*key);
                let sig = fresh_sig(signatures, 1);
                let id = emit_member_fn(
                    Arc::from(format!("{name}.set {key_str}")),
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
                });
            }
            ClassMember::Property {
                key,
                init,
                modifiers,
                ..
            } if modifiers.is_static => {
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
                    );
                    def.prelude.extend(pre);
                    x
                });
                def.statics
                    .push((Arc::from(ctx.interner.resolve(*key)), init_x));
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
            _ => {}
        }
    }
    def
}
