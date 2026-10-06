











use rustc_hash::FxHashSet;
use std::sync::Arc;
use varn_core::ast::operators::AssignOp;
use varn_core::ast::{AstArena, ExprId, ExprKind, StmtId, StmtKind};
use varn_core::AtomInterner;


pub(super) fn fields_assigned_on_every_path(
    body: StmtId,
    arena: &AstArena,
    interner: &AtomInterner,
) -> FxHashSet<Arc<str>> {
    let mut out = FxHashSet::default();
    walk_stmt(body, arena, interner, &mut out);
    out
}

fn this_field_target(e: ExprId, arena: &AstArena, interner: &AtomInterner) -> Option<Arc<str>> {
    match &arena.expr(e).kind {
        ExprKind::Member {
            object,
            property,
            computed: false,
            ..
        } => {
            if !matches!(arena.expr(*object).kind, ExprKind::This) {
                return None;
            }
            match &arena.expr(*property).kind {
                ExprKind::Identifier { name } => Some(Arc::from(interner.resolve(*name))),
                _ => None,
            }
        }
        _ => None,
    }
}

fn walk_expr(e: ExprId, arena: &AstArena, interner: &AtomInterner, out: &mut FxHashSet<Arc<str>>) {
    if let ExprKind::Assign {
        op: AssignOp::Assign,
        target,
        ..
    } = &arena.expr(e).kind
    {
        if let Some(name) = this_field_target(*target, arena, interner) {
            out.insert(name);
        }
    }
}





fn walk_stmt(s: StmtId, arena: &AstArena, interner: &AtomInterner, out: &mut FxHashSet<Arc<str>>) {
    match &arena.stmt(s).kind {
        StmtKind::Block { stmts } => {
            for inner in stmts {
                walk_stmt(*inner, arena, interner, out);
            }
        }
        StmtKind::Expr { expression } => walk_expr(*expression, arena, interner, out),
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => {
            let mut then_set = FxHashSet::default();
            walk_stmt(*consequent, arena, interner, &mut then_set);
            let else_set = match alternate {
                Some(alt) => {
                    let mut s = FxHashSet::default();
                    walk_stmt(*alt, arena, interner, &mut s);
                    s
                }
                
                
                None => FxHashSet::default(),
            };
            out.extend(then_set.intersection(&else_set).cloned());
        }
        
        
        
        _ => {}
    }
}
