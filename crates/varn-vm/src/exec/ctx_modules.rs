use crate::error::{RuntimeError, VmResult};
use crate::heap::HeapObj;
use crate::value::VmValue;
use std::sync::Arc;
use varn_core::ModuleId;
use varn_types::value::{FrozenExport, FrozenModuleObj};
use varn_types::ModuleObj;

use super::ctx::ExecCtx;

impl ExecCtx {
    pub fn convert_to_module_obj(&mut self, id: ModuleId, val: VmValue) -> VmResult<VmValue> {
        if !val.is_heap() {
            return Ok(val);
        }
        let raw_idx = val.as_heap_idx();
        match self.heap.get(raw_idx) {
            Some(crate::heap::HeapObj::Module(_)) => Ok(val),
            Some(crate::heap::HeapObj::Object(obj)) => {
                let obj_ref = obj.borrow();

                let mut keys: Vec<std::rc::Rc<str>> = obj_ref.inner.keys().collect();
                keys.sort();

                let mut export_map = rustc_hash::FxHashMap::default();
                let mut exports = Vec::with_capacity(keys.len());
                for (idx, key) in keys.iter().enumerate() {
                    export_map.insert(key.clone(), idx);
                    let val = obj_ref.get_field_nv(key).unwrap_or(VmValue::null());
                    exports.push(val);
                }
                drop(obj_ref);

                let mut module_obj = ModuleObj::new(id, keys.len());
                module_obj.exports = exports;
                module_obj.export_map = export_map;

                let module_val = self.heap.alloc_module(std::rc::Rc::new(module_obj));
                Ok(module_val)
            }
            _ => Ok(val),
        }
    }

    pub fn load_module(&mut self, specifier: &str) -> VmResult<VmValue> {
        use crate::exec::modules;

        let source_file = self
            .frames
            .last()
            .map(|f| f.closure().proto.chunk.source_file.clone())
            .unwrap_or_else(|| "".to_owned().into());
        let resolved = modules::resolve_specifier_from_path(specifier, &source_file.to_string())?;

        // 1. Linker cache — fastest path.
        if let Some(cached) = self.linker.cached(&resolved) {
            return Ok(cached);
        }

        // 2. Module cache — check for both live Module and FrozenModule.
        if let Some(&cached) = self.modules.get(&resolved) {
            // If frozen, thaw into this VM's heap before returning.
            if cached.is_heap() {
                if let Some(HeapObj::FrozenModule(frozen)) =
                    self.heap.get(cached.as_heap_idx())
                {
                    let frozen = frozen.clone();
                    let thawed = thaw_module(&frozen, &mut self.heap);
                    self.linker.set_done(resolved, thawed);
                    return Ok(thawed);
                }
            }
            return Ok(cached);
        }

        // 3. Native modules — those with a direct Rust implementation in
        //    MODULE_OPS (keyed by full ID: "runtime:math", "std:collections").
        //    std:X modules that are pure Varn wrappers (like "std:math") will
        //    miss here and fall through to the .vn loader path below.
        let native_name = match &resolved {
            ModuleId::Std(name) | ModuleId::Core(name) | ModuleId::Runtime(name) => {
                Some(name.as_ref().to_owned())
            }
            _ => None,
        };
        if let Some(ref name) = native_name {
            if let Some(nv) = varn_builtins::build_module(name, &mut self.heap) {
                let converted = self.convert_to_module_obj(resolved.clone(), nv)?;
                self.modules.insert(resolved.clone(), converted);
                self.linker.set_done(resolved, converted);
                return Ok(converted);
            }
        }

        // 4. Cycle detection via linker graph state (replaces frame-scan).
        if self.linker.is_evaluating(&resolved) {
            // Circular dependency detected. Return the partial module that was
            // pre-inserted before evaluation started. Accessing an uninitialized
            // export from within this cycle constitutes a TDZ violation.
            return self.modules.get(&resolved).copied().ok_or_else(|| {
                RuntimeError::new(format!(
                    "E_BINDING_TDZ: circular dependency on '{specifier}'"
                ))
            });
        }

        // 5. Precompiled map (ahead-of-time compiled dependencies).
        if let Some(proto) = self.precompiled.get(&resolved).cloned() {
            return self.eval_module_proto(resolved, proto);
        }

        // 6. Dynamic loader (FileLoader + StdlibLoader via CompositeLoader).
        let loader = self.loader.clone();
        if let Some(loader) = loader {
            if let Ok(Some(proto)) = loader.load(&resolved) {
                return self.eval_module_proto(resolved, proto);
            }
        }

        Err(RuntimeError::new(format!("module not found: {specifier}")))
    }

    fn eval_module_proto(
        &mut self,
        resolved: ModuleId,
        proto: std::rc::Rc<varn_types::FunctionProto>,
    ) -> VmResult<VmValue> {
        debug_assert!(
            proto.export_names.windows(2).all(|w| w[0] <= w[1]),
            "FunctionProto export_names must be sorted alphabetically (slot contract violated for {})",
            resolved.as_str()
        );

        // Pre-allocate the module namespace so circular imports get a partial object.
        let mut export_map = rustc_hash::FxHashMap::default();
        for (idx, name) in proto.export_names.iter().enumerate() {
            export_map.insert(name.clone(), idx);
        }
        let mut module_obj = ModuleObj::new(resolved.clone(), proto.export_names.len());
        module_obj.export_map = export_map;
        let module_val = self.heap.alloc_module(std::rc::Rc::new(module_obj));
        self.modules.insert(resolved.clone(), module_val);

        // Mark as evaluating in the linker graph.
        self.linker.set_evaluating(resolved.clone());

        let closure = crate::exec::calls::build_closure(proto, &mut self.heap);
        self.push_frame(closure)?;
        let frame_idx = self.frames.len() - 1;
        self.module_exports.insert(frame_idx, module_val);

        let res = match self.run_until(frame_idx) {
            Ok(v) => v,
            Err(e) => {
                // Clean up linker state so the module isn't stuck Evaluating.
                self.linker.cancel_evaluating(&resolved);
                self.modules.remove(&resolved);
                return Err(e);
            }
        };
        if self.vm_suspend.is_some() {
            return Ok(module_val);
        }
        let final_val = self.modules.get(&resolved).copied().unwrap_or(res);
        self.modules.insert(resolved.clone(), final_val);

        // Mark as done in the linker graph.
        self.linker.set_done(resolved, final_val);
        Ok(final_val)
    }
}

/// Freeze a live module value into a heap-index-free `FrozenModuleObj`.
/// Returns `None` if any export is not freezeable (e.g. a VM closure).
/// Only modules built entirely from NativeFns, primitives, and nested objects
/// of the same will produce a `Some` result.
pub fn freeze_module(
    module_val: VmValue,
    id: ModuleId,
    heap: &crate::heap::HeapInner,
) -> Option<Arc<FrozenModuleObj>> {
    let raw_idx = module_val.as_heap_idx();
    let m = match heap.get(raw_idx) {
        Some(HeapObj::Module(m)) => m.clone(),
        _ => return None,
    };

    let mut frozen = FrozenModuleObj::new(id);

    for (key, &slot) in &m.export_map {
        let export_val = m.get_slot(slot).unwrap_or(VmValue::null());
        let frozen_export = freeze_value(export_val, heap)?;
        frozen.push(Arc::from(key.as_ref()), frozen_export);
    }

    Some(Arc::new(frozen))
}

/// Returns `None` if the value contains a VM closure or other non-freezeable type.
fn freeze_value(val: VmValue, heap: &crate::heap::HeapInner) -> Option<FrozenExport> {
    // Primitives fit in the NaN-boxed VmValue itself — no heap index.
    if !val.is_heap() {
        return Some(FrozenExport::Primitive(val));
    }

    match heap.get(val.as_heap_idx()) {
        Some(HeapObj::Str(s)) => Some(FrozenExport::Str(Arc::from(s.as_ref()))),
        Some(HeapObj::NativeFn(name, f)) => Some(FrozenExport::NativeFn(*f, name)),
        Some(HeapObj::Object(obj_ref)) => {
            // Nested namespace object — freeze it recursively.
            // If any child is not freezeable, abort entire module freeze.
            let guard = obj_ref.borrow();
            let mut nested = FrozenModuleObj::new(ModuleId::local_str("<nested>"));
            for (key, nv) in guard.inner.iter() {
                let child = freeze_value(nv, heap)?;
                nested.push(Arc::from(key.as_ref()), child);
            }
            Some(FrozenExport::Nested(Arc::new(nested)))
        }
        _ => {
            // VM closures, classes, arrays, tasks — not freezeable.
            None
        }
    }
}

/// Thaw a `FrozenModuleObj` into a live `ModuleObj` in `heap`.
/// Allocates one heap slot per native function export; primitives are zero-cost.
pub fn thaw_module(
    frozen: &FrozenModuleObj,
    heap: &mut crate::heap::HeapInner,
) -> VmValue {
    let n = frozen.exports.len();
    let mut module_obj = ModuleObj::new(frozen.id.clone(), n);

    for (key, &slot) in &frozen.export_map {
        let nv = thaw_export(&frozen.exports[slot], heap);
        module_obj.exports.resize(slot + 1, VmValue::null());
        module_obj.exports[slot] = nv;
        module_obj.export_map.insert(std::rc::Rc::from(key.as_ref()), slot);
    }

    heap.alloc_module(std::rc::Rc::new(module_obj))
}

fn thaw_export(export: &FrozenExport, heap: &mut crate::heap::HeapInner) -> VmValue {
    match export {
        FrozenExport::Primitive(v) => *v,
        FrozenExport::Str(s) => heap.alloc_str(s),
        FrozenExport::NativeFn(f, name) => heap.alloc_native_fn(*f, name),
        FrozenExport::Nested(nested) => {
            // Thaw nested namespace into a plain Object on the heap.
            let obj_val = heap.alloc_object();
            let raw_idx = obj_val.as_heap_idx();
            // Collect first to avoid borrow conflicts.
            let fields: Vec<(Arc<str>, FrozenExport)> = nested
                .export_map
                .iter()
                .map(|(k, &slot)| (k.clone(), nested.exports[slot].clone()))
                .collect();
            for (key, child_export) in fields {
                let child_nv = thaw_export(&child_export, heap);
                if let Some(HeapObj::Object(o)) = heap.get_by_idx_mut(raw_idx) {
                    o.borrow_mut().set_field_nv(std::rc::Rc::from(key.as_ref()), child_nv);
                }
            }
            obj_val
        }
    }
}
