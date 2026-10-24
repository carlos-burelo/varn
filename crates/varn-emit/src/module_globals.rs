use super::classes::top_level_type_builds;
use super::decl_classify::is_value_symbol;
use super::extensions::collect_extension_names;
use super::namespaces::collect_decl_names;
use super::nested_types::NestedTypes;
use super::tables::NameIndex;
use super::ty::lower_type;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::{AstArena, MetaProperty, Pattern, Program, StmtKind};
use varn_core::AtomInterner;
use varn_sem::bind::BindResult;
use varn_tir::{BackendTy, DynReason, EnumId, EnumInfo, TyTable};

pub(super) fn core_method_ops(bind: &BindResult) -> FxHashMap<(Arc<str>, Arc<str>), u64> {
    let mut out = FxHashMap::default();
    let Some(provider) = varn_modules::provider::get() else {
        return out;
    };
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
            if m.is_static || m.is_async || m.is_generator {
                continue;
            }
            let key: Arc<str> = match (m.kind, m.is_readonly) {
                (varn_sem::types::ClassMemberKind::Method, _) => m.name.clone(),
                (varn_sem::types::ClassMemberKind::Getter, _)
                | (varn_sem::types::ClassMemberKind::Property, true) => {
                    Arc::from(varn_core::AccessorKind::Get.key(&m.name))
                }
                (
                    varn_sem::types::ClassMemberKind::Property
                    | varn_sem::types::ClassMemberKind::Function
                    | varn_sem::types::ClassMemberKind::Variable
                    | varn_sem::types::ClassMemberKind::Constructor
                    | varn_sem::types::ClassMemberKind::Setter
                    | varn_sem::types::ClassMemberKind::Class
                    | varn_sem::types::ClassMemberKind::Interface
                    | varn_sem::types::ClassMemberKind::Namespace
                    | varn_sem::types::ClassMemberKind::Enum
                    | varn_sem::types::ClassMemberKind::Struct,
                    _,
                ) => continue,
            };
            if let Some(id) = provider.native_op_id(varn_core::op_id::CORE_MODULE, cname, &m.name)
            {
                out.insert((Arc::from(cname), key), id);
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
        let kind = &ast_arena.stmt(stmt).kind;
        if let StmtKind::Decl(d) = kind {
            collect_decl_names(d, ast_arena, &mut declared, interner);
        }
        if let StmtKind::Using { declarations, .. } = kind {
            for d in declarations {
                if let Pattern::Identifier { name, .. } = &d.id {
                    declared.insert(Arc::from(interner.resolve(*name)));
                }
            }
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
    enums: &[EnumInfo],
) -> GlobalSlots {
    let interner = &bind.interner;
    let mut global_slots: FxHashMap<Arc<str>, u32> = FxHashMap::default();
    let mut globals: Vec<BackendTy> = Vec::new();
    for property in MetaProperty::ALL {
        debug_assert_eq!(property.slot() as usize, globals.len());
        globals.push(BackendTy::Bool);
        global_slots.insert(Arc::from(property.source_text()), property.slot());
    }
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
                .unwrap_or(BackendTy::Dynamic(DynReason::Unrecorded)),
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
            globals.push(BackendTy::Dynamic(DynReason::Unrecorded));
            global_slots.insert(name, slot);
        }
    }
    let top_types = super::nested_types::top_level_types(
        program,
        ast_arena,
        interner,
        &bind.declared_type_names,
    );
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
        globals.push(BackendTy::Dynamic(DynReason::Unrecorded));
        global_slots.insert(name, slot);
    }
    for (id, info) in enums.iter().enumerate() {
        if !info
            .binding
            .as_ref()
            .is_some_and(|binding| global_slots.contains_key(binding))
        {
            continue;
        }
        for variant in &info.variants {
            let Some(global) = info.variant_global(&variant.name) else {
                continue;
            };
            let held = if variant.payload.is_empty() {
                BackendTy::Enum(EnumId(id as u32))
            } else {
                BackendTy::Dynamic(DynReason::Callable)
            };
            global_slots.insert(global, globals.len() as u32);
            globals.push(held);
        }
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
