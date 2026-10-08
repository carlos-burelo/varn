use super::CallTypeCtx;
use std::sync::Arc;
use varn_binder::resolve_type_node;
use varn_core::ast::{ExprId, ExprKind, TypeNode};
use varn_sem::types::Type;

pub(super) fn infer_new(
    c: &mut CallTypeCtx,
    callee: ExprId,
    type_args: &[TypeNode],
) -> Option<Type> {
    let arena = c.ast_arena;
    let interner = c.interner;
    if let ExprKind::Identifier { name } = &arena.expr(callee).kind {
        let name_str = interner.resolve(*name);
        if !type_args.is_empty() {
            let ctx = c.ctx;
            let mut args = Vec::new();
            for node in type_args {
                args.push(resolve_type_node(node, ctx, c.table));
            }
            return Some(Type::generic(Arc::from(name_str), args, c.table));
        }
        if name_str == varn_core::BuiltinType::Map.name() {
            return Some(Type::generic(
                Arc::from(name_str),
                vec![Type::Dynamic, Type::Dynamic],
                c.table,
            ));
        }
        return Some(Type::named(Arc::from(name_str), c.table));
    }
    None
}
