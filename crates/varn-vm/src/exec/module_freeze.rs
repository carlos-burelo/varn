use crate::heap::HeapObj;
use crate::value::VmValue;
use std::sync::Arc;
use varn_core::ModuleId;
use varn_types::value::{FrozenExport, FrozenModuleObj};
use varn_types::ModuleObj;

pub(crate) fn freeze_module(
    module_val: VmValue,
    id: ModuleId,
    heap: &crate::heap::HeapInner,
) -> Option<Arc<FrozenModuleObj>> {
    let raw_idx = module_val.as_heap();
    let m = match heap.get(raw_idx) {
        Some(HeapObj::Module(m)) => m.clone(),
        Some(
            HeapObj::Str(_)
            | HeapObj::Array(_)
            | HeapObj::Tuple(_)
            | HeapObj::Object(_)
            | HeapObj::Record(_)
            | HeapObj::Buffer(_)
            | HeapObj::FrozenModule(_)
            | HeapObj::VmClosure(_)
            | HeapObj::Class(_)
            | HeapObj::NativeFn(..)
            | HeapObj::BoundMethod(_)
            | HeapObj::Map(_)
            | HeapObj::Set(_)
            | HeapObj::Task(_)
            | HeapObj::TaskHandle(_)
            | HeapObj::Range(_)
            | HeapObj::Symbol(_)
            | HeapObj::EnumVariant(_)
            | HeapObj::BigInt(_)
            | HeapObj::Decimal(_)
            | HeapObj::Char(_)
            | HeapObj::Generator(_)
            | HeapObj::Spread(_),
        )
        | None => return None,
    };

    let mut frozen_exports = Vec::with_capacity(m.exports.len());
    for &export_val in &m.exports {
        let frozen_export = freeze_value(export_val, heap)?;
        frozen_exports.push(frozen_export);
    }

    let mut frozen_export_map = rustc_hash::FxHashMap::default();
    for (key, &slot) in &m.export_map {
        frozen_export_map.insert(Arc::from(key.as_ref()), slot);
    }

    Some(Arc::new(FrozenModuleObj {
        id,
        exports: frozen_exports,
        export_map: frozen_export_map,
    }))
}

fn freeze_value(val: VmValue, heap: &crate::heap::HeapInner) -> Option<FrozenExport> {
    if !val.is_heap() {
        return Some(FrozenExport::Primitive(val));
    }

    match heap.get(val.as_heap()) {
        Some(HeapObj::Str(s)) => Some(FrozenExport::Str(Arc::from(s.as_ref()))),
        Some(HeapObj::NativeFn(f, name)) => Some(FrozenExport::NativeFn(*f, name)),
        Some(HeapObj::Class(_)) => None,
        Some(HeapObj::VmClosure(_)) => None,
        Some(HeapObj::Object(obj_ref)) => {
            let guard = obj_ref.borrow();
            let mut nested = FrozenModuleObj::new(ModuleId::local_str("<nested>"));
            for (key, nv) in guard.iter() {
                let child = freeze_value(nv, heap)?;
                nested.push(Arc::from(key.as_ref()), child);
            }
            Some(FrozenExport::Nested(Arc::new(nested)))
        }
        Some(
            HeapObj::Array(_)
            | HeapObj::Tuple(_)
            | HeapObj::Record(_)
            | HeapObj::Buffer(_)
            | HeapObj::Module(_)
            | HeapObj::FrozenModule(_)
            | HeapObj::BoundMethod(_)
            | HeapObj::Map(_)
            | HeapObj::Set(_)
            | HeapObj::Task(_)
            | HeapObj::TaskHandle(_)
            | HeapObj::Range(_)
            | HeapObj::Symbol(_)
            | HeapObj::EnumVariant(_)
            | HeapObj::BigInt(_)
            | HeapObj::Decimal(_)
            | HeapObj::Char(_)
            | HeapObj::Generator(_)
            | HeapObj::Spread(_),
        )
        | None => None,
    }
}

pub(crate) fn thaw_module(frozen: &FrozenModuleObj, heap: &mut crate::heap::HeapInner) -> VmValue {
    let n = frozen.exports.len();
    let mut module_obj = ModuleObj::new(frozen.id.clone(), n);
    module_obj.exports.resize(n, VmValue::null());

    for (key, &slot) in &frozen.export_map {
        let nv = thaw_export(&frozen.exports[slot], heap);
        module_obj.exports[slot] = nv;
        module_obj
            .export_map
            .insert(std::sync::Arc::from(key.as_ref()), slot);
    }

    heap.alloc_module(std::rc::Rc::new(module_obj))
}

fn thaw_export(export: &FrozenExport, heap: &mut crate::heap::HeapInner) -> VmValue {
    match export {
        FrozenExport::Primitive(v) => *v,
        FrozenExport::Str(s) => heap.alloc_str(s),
        FrozenExport::NativeFn(f, name) => heap.alloc_native_fn(*f, name),
        FrozenExport::Class(cls) => VmValue::from_heap(heap.alloc(HeapObj::Class(cls.clone()))),
        FrozenExport::Nested(nested) => {
            let obj_val = heap.alloc_object();
            let raw_idx = obj_val.as_heap();

            let fields: Vec<(Arc<str>, FrozenExport)> = nested
                .export_map
                .iter()
                .map(|(k, &slot)| (k.clone(), nested.exports[slot].clone()))
                .collect();
            for (key, child_export) in fields {
                let child_nv = thaw_export(&child_export, heap);
                if let Some(HeapObj::Object(o)) = heap.get_mut(raw_idx) {
                    o.set_field(std::sync::Arc::from(key.as_ref()), child_nv);
                }
            }
            obj_val
        }
    }
}
