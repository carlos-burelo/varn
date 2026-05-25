use crate::binder::BindResult;
use crate::symbol::SymbolKind;
use crate::types::{FunctionType, Type};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::TypeKind;

pub(super) struct EnrichContext {
    pub fn_map: FxHashMap<Rc<str>, Type>,
    pub fn_type_params: FxHashMap<Rc<str>, Vec<Rc<str>>>,
    pub class_methods: FxHashMap<Rc<str>, FxHashMap<Rc<str>, Type>>,
}

pub(super) fn build_enrich_context(bind: &BindResult) -> (EnrichContext, FxHashMap<Rc<str>, Type>) {
    let symbols = bind.arena.all();
    let mut fn_map = FxHashMap::with_capacity_and_hasher(symbols.len(), Default::default());
    let mut sym_map = FxHashMap::with_capacity_and_hasher(symbols.len(), Default::default());
    let mut fn_type_params = FxHashMap::default();

    for sym in symbols.iter() {
        if let Some(ty) = &sym.ty {
            sym_map.insert(sym.name.clone(), ty.clone());

            if sym.kind == SymbolKind::Function {
                if let Type(TypeKind::Fn(FunctionType { return_type, .. }), _) = ty {
                    let raw = return_type.as_ref().clone();
                    if !raw.is_dynamic() {
                        let call_ret = if sym.is_async
                            && !matches!(&raw.0, TypeKind::Generic(name, _, _) if name.as_ref() == varn_core::IntrinsicType::Task.as_str())
                        {
                            Type::generic(
                                varn_core::IntrinsicType::Task.as_str().to_owned(),
                                vec![raw],
                            )
                        } else {
                            raw
                        };
                        fn_map.insert(sym.name.clone(), call_ret);
                    }
                }
            }
        }

        if sym.kind == SymbolKind::Function && !sym.type_params.is_empty() {
            fn_type_params.insert(sym.name.clone(), sym.type_params.clone());
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
