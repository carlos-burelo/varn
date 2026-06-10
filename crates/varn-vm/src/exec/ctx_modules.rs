use crate::error::{RuntimeError, VmResult};
use crate::value::VmValue;
use varn_core::ModuleId;
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

                let id_str = id.as_str();
                let expected_keys = get_cached_exports(&id_str);

                let keys: Vec<std::rc::Rc<str>> = if let Some(parsed) = expected_keys {
                    parsed
                } else {
                    let mut k: Vec<std::rc::Rc<str>> = obj_ref.inner.keys().collect();
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
            .map(|f| f.closure.proto.chunk.source_file.clone())
            .unwrap_or_else(|| "".to_owned().into());
        let resolved = modules::resolve_specifier_from_path(specifier, &source_file.to_string())?;

        // 1. Linker cache — fastest path.
        if let Some(cached) = self.linker.cached(&resolved) {
            return Ok(cached);
        }
        if let Some(&cached) = self.modules.get(&resolved) {
            return Ok(cached);
        }

        // 2. Native modules — those with a direct Rust implementation in
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

        // 3. Cycle detection via linker graph state (replaces frame-scan).
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

        // 4. Precompiled map (ahead-of-time compiled dependencies).
        if let Some(proto) = self.precompiled.get(&resolved).cloned() {
            return self.eval_module_proto(resolved, proto);
        }

        // 5. Dynamic loader (FileLoader + StdlibLoader via CompositeLoader).
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

fn get_cached_exports(module_id: &str) -> Option<Vec<std::rc::Rc<str>>> {
    let spec = varn_builtins::spec_for(module_id)?;
    if spec.exports.is_empty() {
        None
    } else {
        Some(spec.exports.iter().map(|&s| std::rc::Rc::from(s)).collect())
    }
}
