use super::classes::top_level_type_builds;
use super::decl_classify::is_value_symbol;
use super::namespaces::{collect_decl_names, collect_extension_names};
use super::nested_types::NestedTypes;
use super::tables::NameIndex;
use super::ty::lower_type;
use crate::binder::BindResult;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::{AstArena, Program, StmtKind};
use varn_core::AtomInterner;
use varn_tir::{BackendTy, DynReason, TyTable};

pub(super) fn core_method_ops(bind: &BindResult) -> FxHashSet<(Arc<str>, Arc<str>)> {
    let mut out = FxHashSet::default();
    let Some(core) = bind.core.as_ref() else {
        return out;
    };
    for tag in varn_core::op_id::CORE_CLASSES {
        let Some(cname) = varn_core::op_id::core_class_name(tag) else {
            continue;
        };
        let Some(info) = core.class_members.get(cname) else {
            continue;
        };
        for m in &info.members {
            if matches!(m.kind, crate::types::ClassMemberKind::Method)
                && !m.is_static
                && !m.is_async
                && !m.is_generator
            {
                out.insert((Arc::from(cname), m.name.clone()));
            }
        }
    }
    out
}

pub(super) fn collect_declared(
    program: &Program,
    ast_arena: &AstArena,
    interner: &AtomInterner,
) -> FxHashSet<Arc<str>> {
    let mut declared: FxHashSet<Arc<str>> = FxHashSet::default();
    for &stmt in &program.body {
        if let StmtKind::Decl(d) = &ast_arena.stmt(stmt).kind {
            collect_decl_names(d, ast_arena, &mut declared, interner);
        }
    }
    collect_extension_names(program, ast_arena, &mut declared, interner);
    declared
}

pub(super) struct GlobalSlots {
    pub(super) slots: FxHashMap<Arc<str>, u32>,
    pub(super) globals: Vec<BackendTy>,
    pub(super) names: Vec<Arc<str>>,
    pub(super) nested: NestedTypes,
    pub(super) first_nested_ordinal: u32,
}

pub(super) fn assign_global_slots(
    program: &Program,
    ast_arena: &AstArena,
    bind: &BindResult,
    types: &mut TyTable,
    names: &NameIndex,
    declared: &FxHashSet<Arc<str>>,
) -> GlobalSlots {
    let interner = &bind.interner;
    let mut global_slots: FxHashMap<Arc<str>, u32> = FxHashMap::default();
    let mut globals: Vec<BackendTy> = Vec::new();
    for sym in bind.global_symbols() {
        if !is_value_symbol(sym.kind) {
            continue;
        }
        let sym_name: Arc<str> = Arc::from(interner.resolve(sym.name));
        if !declared.contains(&sym_name) {
            continue;
        }
        if global_slots.contains_key(&sym_name) {
            continue;
        }
        let slot = globals.len() as u32;
        globals.push(
            sym.ty
                .as_ref()
                .map(|t| lower_type(t, &bind.ty_table, interner, types, names))
                .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated)),
        );
        global_slots.insert(sym_name, slot);
    }
    {
        let mut extra: Vec<Arc<str>> = declared
            .iter()
            .filter(|n| !global_slots.contains_key(n.as_ref()))
            .cloned()
            .collect();
        extra.sort();
        for name in extra {
            let slot = globals.len() as u32;
            globals.push(BackendTy::Dynamic(DynReason::Unannotated));
            global_slots.insert(name, slot);
        }
    }
    let top_types = super::nested_types::top_level_types(program, ast_arena, interner);
    let first_nested_ordinal = top_level_type_builds(program, ast_arena);
    let nested = NestedTypes::collect(
        bind,
        &top_types,
        &global_slots,
        globals.len() as u32,
        first_nested_ordinal,
    );
    for (name, slot) in nested.new_globals(&global_slots) {
        debug_assert_eq!(slot as usize, globals.len());
        globals.push(BackendTy::Dynamic(DynReason::Unannotated));
        global_slots.insert(name, slot);
    }
    let mut global_names: Vec<Arc<str>> = vec![Arc::from(""); globals.len()];
    for (name, &slot) in &global_slots {
        global_names[slot as usize] = name.clone();
    }
    GlobalSlots {
        slots: global_slots,
        globals,
        names: global_names,
        nested,
        first_nested_ordinal,
    }
}
