use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::{AstArena, Decl, ExprKind, Pattern, Program, StmtKind};
use varn_core::AtomInterner;
use varn_tir::{walk, LocalId, Resolution, TirModule};

fn top_level_variables(
    program: &Program,
    ast_arena: &AstArena,
    interner: &AtomInterner,
) -> FxHashSet<Arc<str>> {
    let mut names = FxHashSet::default();
    for &stmt in &program.body {
        let StmtKind::Decl(decl) = &ast_arena.stmt(stmt).kind else {
            continue;
        };
        let Decl::Variable(v) = decl.as_ref() else {
            continue;
        };
        if v.is_declare {
            continue;
        }
        for d in &v.declarators {
            let Pattern::Identifier { name, .. } = &d.id else {
                continue;
            };
            let class_valued = d
                .init
                .is_some_and(|e| matches!(ast_arena.expr(e).kind, ExprKind::ClassExpr { .. }));
            if !class_valued {
                names.insert(Arc::from(interner.resolve(*name)));
            }
        }
    }
    names
}

pub(super) fn promote(
    tir: &mut TirModule,
    program: &Program,
    ast_arena: &AstArena,
    interner: &AtomInterner,
) {
    let variables = top_level_variables(program, ast_arena, interner);
    let exported: FxHashSet<&str> = tir
        .exports
        .iter()
        .filter(|e| e.reexport_from.is_none())
        .map(|e| e.local.as_ref())
        .collect();
    let candidates: Vec<u32> = tir
        .global_names
        .iter()
        .enumerate()
        .filter(|(_, name)| variables.contains(*name) && !exported.contains(name.as_ref()))
        .map(|(slot, _)| slot as u32)
        .collect();
    if candidates.is_empty() {
        return;
    }

    let mut escaping: FxHashSet<u32> = FxHashSet::default();
    let mut note = |e: &mut varn_tir::TirExpr| {
        if let Resolution::GlobalSlot(slot) = e.res {
            escaping.insert(slot);
        }
    };
    for f in &mut tir.functions {
        walk::function_exprs_mut(f, &mut note);
    }
    for def in &mut tir.class_defs {
        walk::class_def_exprs_mut(def, &mut note);
    }

    let top = &mut tir.top_level;
    let mut promoted: FxHashMap<u32, LocalId> = FxHashMap::default();
    for slot in candidates.into_iter().filter(|s| !escaping.contains(s)) {
        promoted.insert(slot, LocalId(top.locals.len() as u32));
        top.locals.push(tir.globals[slot as usize].clone());
    }
    walk::function_exprs_mut(top, &mut |e| {
        if let Resolution::GlobalSlot(slot) = e.res
            && let Some(&local) = promoted.get(&slot)
        {
            e.res = Resolution::Local(local);
        }
    });
}
