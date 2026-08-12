use crate::error::{RuntimeError, VmResult};
use crate::heap::HeapObj;
use crate::value::VmValue;
use std::sync::Arc;
use varn_core::ModuleId;
use varn_types::value::{FrozenExport, FrozenModuleObj};
use varn_types::ModuleObj;

use super::ctx::ExecCtx;

fn canonical_id_str(id: &ModuleId) -> String {
    match id {
        ModuleId::Core(s) => {
            if s.starts_with("core:") {
                s.to_string()
            } else {
                format!("core:{s}")
            }
        }
        ModuleId::Std(s) => {
            if s.starts_with("std:") {
                s.to_string()
            } else {
                format!("std:{s}")
            }
        }
        ModuleId::Runtime(s) => {
            if s.starts_with("runtime:") {
                s.to_string()
            } else {
                format!("runtime:{s}")
            }
        }
        _ => id.as_str(),
    }
}

impl ExecCtx {
    pub(crate) fn convert_to_module_obj(
        &mut self,
        id: ModuleId,
        val: VmValue,
    ) -> VmResult<VmValue> {
        if !val.is_heap() {
            return Ok(val);
        }
        let raw_idx = val.as_heap_idx();
        match self.heap.get(raw_idx) {
            Some(crate::heap::HeapObj::Module(_)) => Ok(val),
            Some(crate::heap::HeapObj::Object(obj)) => {
                let obj_ref = obj.borrow();

                let id_str = canonical_id_str(&id);
                let expected_keys = get_cached_exports(&id_str);

                let keys: Vec<std::rc::Rc<str>> = if let Some(parsed) = expected_keys {
                    parsed
                } else {
                    let mut k: Vec<std::rc::Rc<str>> = obj_ref.keys().collect();
                    k.sort();
                    k
                };

                let mut export_map = rustc_hash::FxHashMap::default();
                let mut exports = Vec::with_capacity(keys.len());
                for (idx, key) in keys.iter().enumerate() {
                    export_map.insert(key.clone(), idx);
                    let val = obj_ref.get_field_nv(key).unwrap_or(VmValue::null());
                    exports.push(val);
                }

                let mut module_obj = ModuleObj::new(id, keys.len());
                module_obj.exports = exports;
                module_obj.export_map = export_map;

                let module_val = self.heap.alloc_module(std::rc::Rc::new(module_obj));
                Ok(module_val)
            }
            _ => Ok(val),
        }
    }

    pub(crate) fn load_module(&mut self, specifier: &str) -> VmResult<VmValue> {
        let source_file = self
            .frames
            .last()
            .map(|f| f.closure().proto.chunk.source_file.clone())
            .unwrap_or_else(|| "".to_owned().into());
        self.load_module_from_source(specifier, &source_file.to_string())
    }

    pub(crate) fn load_module_from_source(
        &mut self,
        specifier: &str,
        source_file: &str,
    ) -> VmResult<VmValue> {
        use crate::exec::modules;

        let resolved = modules::resolve_specifier_from_path(specifier, source_file)?;

        if let Some(cached) = self.linker.cached(&resolved) {
            return Ok(cached);
        }

        if let Some(&cached) = self.modules.get(&resolved) {
            if cached.is_heap() {
                if let Some(HeapObj::FrozenModule(frozen)) = self.heap.get(cached.as_heap_idx()) {
                    let frozen = frozen.clone();
                    let thawed = thaw_module(&frozen, &mut self.heap);
                    self.linker.set_done(resolved, thawed);
                    return Ok(thawed);
                }
            }
            return Ok(cached);
        }

        let spec_str = canonical_id_str(&resolved);
        let is_pure = varn_builtins::spec_for(&spec_str).map_or(false, |s| s.pure);
        let builtin_nv = if !is_pure {
            varn_builtins::build_module(&spec_str, &mut self.heap).or_else(|| match &resolved {
                ModuleId::Std(name) | ModuleId::Core(name) | ModuleId::Runtime(name) => {
                    let is_p = varn_builtins::spec_for(name.as_ref()).map_or(false, |s| s.pure);
                    if !is_p {
                        varn_builtins::build_module(name.as_ref(), &mut self.heap)
                    } else {
                        None
                    }
                }
                _ => None,
            })
        } else {
            None
        };
        if let Some(nv) = builtin_nv {
            let converted = self.convert_to_module_obj(resolved.clone(), nv)?;
            self.modules.insert(resolved.clone(), converted);
            self.linker.set_done(resolved, converted);
            return Ok(converted);
        }

        if self.linker.is_evaluating(&resolved) {
            return self.modules.get(&resolved).copied().ok_or_else(|| {
                RuntimeError::new(format!(
                    "E_BINDING_TDZ: circular dependency on '{specifier}'"
                ))
            });
        }

        if let Some(proto) = self.precompiled.get(&resolved).cloned() {
            let result = self.eval_module_proto(resolved.clone(), proto);
            // For pure modules, also run native class builders so that classes
            // declared via `export declare class` (methods defined in Rust, not
            // in Varn source) get their method tables populated.
            if is_pure {
                varn_builtins::build_module(&spec_str, &mut self.heap);
            }
            return result;
        }

        let loader = self.loader.clone();
        if let Some(loader) = loader {
            if let Ok(Some(proto)) = loader.load(&resolved) {
                let result = self.eval_module_proto(resolved.clone(), proto);
                if is_pure {
                    varn_builtins::build_module(&spec_str, &mut self.heap);
                }
                return result;
            }
        }

        Err(RuntimeError::new(format!("module not found: {specifier}")))
    }

    fn eval_module_proto(
        &mut self,
        resolved: ModuleId,
        mut proto: std::rc::Rc<varn_types::FunctionProto>,
    ) -> VmResult<VmValue> {
        // Every module reaches the VM through here — `precompiled`, `FileLoader`,
        // the stdlib bundle — and every VM (main, isolate worker, the
        // module-freezing scratch VM) has its own `GlobalStore`. Resolving here
        // is what makes the name-keyed global opcodes unreachable at runtime;
        // `make_mut` keeps a proto shared with the thread-local stdlib cache or
        // the `precompiled` map from inheriting another VM's indices.
        crate::globals::resolve_shared(&mut proto, &mut self.globals);

        debug_assert!(
            proto.export_names.windows(2).all(|w| w[0] <= w[1]),
            "FunctionProto export_names must be sorted alphabetically (slot contract violated for {})",
            resolved.as_str()
        );

        let mut export_map = rustc_hash::FxHashMap::default();
        for (idx, name) in proto.export_names.iter().enumerate() {
            export_map.insert(name.clone(), idx);
        }
        let mut module_obj = ModuleObj::new(resolved.clone(), proto.export_names.len());
        module_obj.export_map = export_map;
        let module_val = self.heap.alloc_module(std::rc::Rc::new(module_obj));
        self.modules.insert(resolved.clone(), module_val);

        self.linker.set_evaluating(resolved.clone());

        let closure = crate::exec::calls::build_closure(proto, &mut self.heap, self.settings);
        self.push_frame(closure)?;
        let frame_idx = self.frames.len() - 1;
        self.module_exports.insert(frame_idx, module_val);

        let res = match self.run_until(frame_idx) {
            Ok(v) => v,
            Err(e) => {
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

        self.linker.set_done(resolved, final_val);
        Ok(final_val)
    }
}

fn get_cached_exports(module_id: &str) -> Option<Vec<std::rc::Rc<str>>> {
    let spec = varn_builtins::spec_for(module_id)?;
    if spec.exports.is_empty() {
        None
    } else {
        Some(spec.exports.iter().map(|&s| std::rc::Rc::from(s)).collect())
    }
}

pub(crate) fn freeze_module(
    module_val: VmValue,
    id: ModuleId,
    heap: &crate::heap::HeapInner,
) -> Option<Arc<FrozenModuleObj>> {
    let raw_idx = module_val.as_heap_idx();
    let m = match heap.get(raw_idx) {
        Some(HeapObj::Module(m)) => m.clone(),
        _ => return None,
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

    match heap.get(val.as_heap_idx()) {
        Some(HeapObj::Str(s)) => Some(FrozenExport::Str(Arc::from(s.as_ref()))),
        Some(HeapObj::NativeFn(f, name)) => Some(FrozenExport::NativeFn(*f, name)),
        Some(HeapObj::Class(_)) => None, // Cannot freeze Class safely across VM instances
        Some(HeapObj::VmClosure(_)) => None, // Cannot freeze VmClosure safely across VM instances
        Some(HeapObj::Object(obj_ref)) => {
            let guard = obj_ref.borrow();
            let mut nested = FrozenModuleObj::new(ModuleId::local_str("<nested>"));
            for (key, nv) in guard.iter() {
                let child = freeze_value(nv, heap)?;
                nested.push(Arc::from(key.as_ref()), child);
            }
            Some(FrozenExport::Nested(Arc::new(nested)))
        }
        _ => None,
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
            .insert(std::rc::Rc::from(key.as_ref()), slot);
    }

    heap.alloc_module(std::rc::Rc::new(module_obj))
}

fn thaw_export(export: &FrozenExport, heap: &mut crate::heap::HeapInner) -> VmValue {
    match export {
        FrozenExport::Primitive(v) => *v,
        FrozenExport::Str(s) => heap.alloc_str(s),
        FrozenExport::NativeFn(f, name) => heap.alloc_native_fn(*f, name),
        FrozenExport::Class(cls) => VmValue::from_heap_idx(heap.alloc(HeapObj::Class(cls.clone()))),
        FrozenExport::VmClosure(payload) => {
            if let Some(wrapper) = payload
                .as_any()
                .downcast_ref::<crate::closure::VmClosurePayload>()
            {
                VmValue::from_heap_idx(heap.alloc(HeapObj::VmClosure(wrapper.0.clone())))
            } else {
                VmValue::null()
            }
        }
        FrozenExport::Nested(nested) => {
            let obj_val = heap.alloc_object();
            let raw_idx = obj_val.as_heap_idx();

            let fields: Vec<(Arc<str>, FrozenExport)> = nested
                .export_map
                .iter()
                .map(|(k, &slot)| (k.clone(), nested.exports[slot].clone()))
                .collect();
            for (key, child_export) in fields {
                let child_nv = thaw_export(&child_export, heap);
                if let Some(HeapObj::Object(o)) = heap.get_by_idx_mut(raw_idx) {
                    o.set_field_nv(std::rc::Rc::from(key.as_ref()), child_nv);
                }
            }
            obj_val
        }
    }
}
