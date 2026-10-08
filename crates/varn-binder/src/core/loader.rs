use std::sync::Arc;
use varn_modules::spec::CORE_PREFIX;
use varn_sem::bind::BindResult;
use varn_sem::cores::{CoreExports, CoreMembers};
use varn_sem::resolver::ImportResolver;

pub fn is_core_file(filename: &str) -> bool {
    filename.contains("varn-builtins") || filename.starts_with(CORE_PREFIX)
}

pub fn module_globals(filename: &str, resolver: &dyn ImportResolver) -> Option<Arc<CoreExports>> {
    (!is_core_file(filename)).then(|| resolver.core_exports())
}

pub fn merge_core_members(bind: &mut BindResult, resolver: &dyn ImportResolver) {
    let core = resolver.core_members();
    bind.interner.absorb(core.table.names());
    Arc::make_mut(&mut bind.ty_table).absorb(&core.table);
    bind.core = Some(core);
}

pub fn build_core_exports(resolver: &dyn ImportResolver) -> CoreExports {
    let mut core = CoreExports::default();
    for spec in varn_modules::core_module_ids() {
        let exports = resolver.stdlib_exports(spec);
        let table = Arc::make_mut(&mut core.table);
        table.absorb(&exports.table);
        let origin = table.intern_name(spec);
        for (k, v) in exports.iter() {
            let mut symbol = v.clone();
            symbol.origin_module = symbol.origin_module.or(Some(origin));
            core.symbols.insert(Arc::from(k.as_str()), symbol);
        }
    }
    core
}

pub fn build_core_members(resolver: &dyn ImportResolver) -> CoreMembers {
    let mut members = CoreMembers::default();
    for spec in varn_modules::core_module_ids() {
        if let Some(rb) = resolver.stdlib_bind(spec) {
            Arc::make_mut(&mut members.table).absorb(&rb.ty_table);
            let scope = rb.scopes.get(rb.global_scope);
            for (name, &sid) in &scope.bindings {
                let sym = rb.arena.get(sid);
                if !sym.type_params.is_empty() {
                    let name_rc: Arc<str> = Arc::from(rb.interner.resolve(*name));
                    let tps: Vec<Arc<str>> = sym
                        .type_params
                        .iter()
                        .map(|a| Arc::from(rb.interner.resolve(*a)))
                        .collect();
                    members.class_type_params.insert(name_rc, tps);
                }
            }

            for (k, v) in &rb.type_members.classes {
                let mut v = v.clone();
                v.is_builtin_or_intrinsic = true;
                members.class_members.insert(k.clone(), v);
            }
            for (k, v) in &rb.type_members.interfaces {
                members.interface_members.insert(k.clone(), v.clone());
            }
            for (k, v) in &rb.type_members.enums {
                members.enum_members.insert(k.clone(), v.clone());
            }
            for (k, v) in &rb.type_members.namespaces {
                members.namespace_members.insert(k.clone(), v.clone());
            }
            for (k, v) in &rb.class_methods {
                members.class_methods.insert(k.clone(), v.clone());
            }
            for (k, v) in &rb.type_members.flattened {
                members.flattened_members.insert(k.clone(), v.clone());
            }
            for (k, v) in &rb.class_parents {
                members.class_parents.insert(k.clone(), v.clone());
            }
        }
    }
    members
}
