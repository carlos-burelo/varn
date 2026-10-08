use super::body::FnEmitter;
use super::decl_classify::{
    anon_class_of, class_decl, enum_decl, free_function, namespace_decl, variable_decl,
};
use super::decorators::emit_fn_decorator_app;
use super::module_ctx::MCtx;
use super::namespaces::{emit_namespace_object, ns_nested_types};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::{AstArena, AstId, Program, StmtKind};
use varn_core::{Atom, AtomInterner};
use varn_tir::{BackendTy, Signature, TirFunction, TirStmt, TyTable};
pub(super) fn lower_top_level(
    program: &Program,
    ast_arena: &AstArena,
    ctx: &MCtx,
    fn_index: &FxHashMap<Atom, (u32, u32)>,
    global_slots: &FxHashMap<Arc<str>, u32>,
    interner: &AtomInterner,
    shadowed: &rustc_hash::FxHashSet<u32>,
    types: &mut TyTable,
    signatures: &mut Vec<Signature>,
    expr_table: &FxHashMap<AstId, varn_sem::output::TypeEntry>,
    closures: &mut Vec<TirFunction>,
    base: u32,
) -> (Vec<BackendTy>, bool, Vec<TirStmt>) {
    let mut top_body = Vec::new();
    let mut top = FnEmitter::new(
        ast_arena,
        expr_table,
        types,
        ctx.as_module_ctx(),
        signatures,
        closures,
        base,
        vec![],
    )
    .into_top_level();
    let mut class_ord: u32 = 0;
    for &stmt in &program.body {
        match &ast_arena.stmt(stmt).kind {
            StmtKind::Decl(d) if class_decl(d).is_some() || enum_decl(d).is_some() => {
                top_body.push(TirStmt::BuildClass(class_ord));
                class_ord += 1;
            }
            StmtKind::Decl(d) if namespace_decl(d).is_some() => {
                let ns = namespace_decl(d).unwrap();
                for _ in ns_nested_types(ns) {
                    top_body.push(TirStmt::BuildClass(class_ord));
                    class_ord += 1;
                }
                emit_namespace_object(
                    ns,
                    fn_index,
                    global_slots,
                    interner,
                    shadowed,
                    &mut top,
                    &mut top_body,
                );
            }
            StmtKind::Decl(d) if anon_class_of(d, ast_arena).is_some() => {
                top_body.push(TirStmt::BuildClass(class_ord));
                class_ord += 1;
                top_body.extend(top.lower_stmt_as_block(stmt));
            }
            StmtKind::Decl(d)
                if free_function(d).is_some_and(|f| {
                    f.decorators.iter().any(|dd| {
                        !varn_core::ast::decorators::is_active_builtin(
                            ast_arena,
                            interner,
                            |off| shadowed.contains(&off),
                            dd,
                        )
                    })
                }) =>
            {
                if let Some(f) = free_function(d) {
                    emit_fn_decorator_app(
                        f,
                        global_slots,
                        interner,
                        shadowed,
                        &mut top,
                        &mut top_body,
                    );
                }
            }
            StmtKind::Decl(d) if variable_decl(d).is_none() => {}
            StmtKind::Block { .. }
            | StmtKind::Empty
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
            | StmtKind::Debugger => top_body.extend(top.lower_stmt_as_block(stmt)),
        }
    }
    (std::mem::take(&mut top.locals), top.saw_await(), top_body)
}
