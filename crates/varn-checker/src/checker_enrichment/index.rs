use crate::binder::BindResult;
use crate::symbol::SymbolKind;
use crate::types::{FunctionType, Type};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::TypeKind;

pub(super) struct EnrichContext {
    pub fn_map: FxHashMap<Arc<str>, Type>,
    pub fn_type_params: FxHashMap<Arc<str>, Vec<Arc<str>>>,
    pub class_methods: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Type>>,
}

pub(super) fn build_enrich_context(
    bind: &BindResult,
) -> (EnrichContext, FxHashMap<Arc<str>, Type>) {
    let symbols = bind.arena.all();
    let mut fn_map = FxHashMap::with_capacity_and_hasher(symbols.len(), Default::default());
    let mut sym_map = FxHashMap::with_capacity_and_hasher(symbols.len(), Default::default());
    let mut fn_type_params = FxHashMap::default();

    for sym in symbols.iter() {
        let name_rc: Arc<str> = Arc::from(bind.interner.resolve(sym.name));
        if let Some(ty) = &sym.ty {
            sym_map.insert(name_rc.clone(), ty.clone());

            if sym.kind == SymbolKind::Function {
                if let TypeKind::Fn(fid) = bind.ty_table.get(ty.0) {
                    let FunctionType { return_type, .. } = bind.ty_table.get_function(fid);
                    // Already `Task<R>` for async functions — the binder wrapped
                    // it when it built the type.
                    let raw = Type(*return_type, false);
                    if !raw.is_dynamic() {
                        fn_map.insert(name_rc.clone(), raw);
                    }
                }
            }
        }

        if sym.kind == SymbolKind::Function && !sym.type_params.is_empty() {
            let tps: Vec<Arc<str>> = sym
                .type_params
                .iter()
                .map(|a| Arc::from(bind.interner.resolve(*a)))
                .collect();
            fn_type_params.insert(name_rc, tps);
        }
    }

    let mut class_methods = bind.class_methods.clone();
    if let Some(b) = &bind.core {
        for (k, v) in &b.class_methods {
            class_methods.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }

    let ctx = EnrichContext {
        fn_map,
        fn_type_params,
        class_methods,
    };
    (ctx, sym_map)
}
