use crate::error::{RuntimeError, VmResult};
use crate::heap::HeapObj;
use crate::value::VmValue;
use varn_core::ModuleId;
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
        ModuleId::Local(_) | ModuleId::Package { .. } => id.as_str(),
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
        let raw_idx = val.as_heap();
        match self.heap.get(raw_idx) {
            Some(crate::heap::HeapObj::Module(_)) => Ok(val),
            Some(crate::heap::HeapObj::Object(obj)) => {
                let obj_ref = obj.borrow();

                let id_str = canonical_id_str(&id);
                let expected_keys = get_cached_exports(&id_str);

                let keys: Vec<std::sync::Arc<str>> = if let Some(parsed) = expected_keys {
                    parsed
                } else {
                    let mut k: Vec<std::sync::Arc<str>> = obj_ref.keys().collect();
                    k.sort();
                    k
                };

                let mut export_map = rustc_hash::FxHashMap::default();
                let mut exports = Vec::with_capacity(keys.len());
                for (idx, key) in keys.iter().enumerate() {
                    export_map.insert(key.clone(), idx);
                    let val = obj_ref.get_field(key).unwrap_or(VmValue::null());
                    exports.push(val);
                }

                let mut module_obj = ModuleObj::new(id, keys.len());
                module_obj.exports = exports;
                module_obj.export_map = export_map;

                let module_val = self.heap.alloc_module(std::rc::Rc::new(module_obj));
                Ok(module_val)
            }
            Some(
                crate::heap::HeapObj::Str(_)
                | crate::heap::HeapObj::Array(_)
                | crate::heap::HeapObj::Tuple(_)
                | crate::heap::HeapObj::Record(_)
                | crate::heap::HeapObj::Buffer(_)
                | crate::heap::HeapObj::FrozenModule(_)
                | crate::heap::HeapObj::VmClosure(_)
                | crate::heap::HeapObj::Class(_)
                | crate::heap::HeapObj::NativeFn(..)
                | crate::heap::HeapObj::BoundMethod(_)
                | crate::heap::HeapObj::Map(_)
                | crate::heap::HeapObj::Set(_)
                | crate::heap::HeapObj::Task(_)
                | crate::heap::HeapObj::TaskHandle(_)
                | crate::heap::HeapObj::Range(_)
                | crate::heap::HeapObj::Symbol(_)
                | crate::heap::HeapObj::EnumVariant(_)
                | crate::heap::HeapObj::BigInt(_)
                | crate::heap::HeapObj::Decimal(_)
                | crate::heap::HeapObj::Char(_)
                | crate::heap::HeapObj::Generator(_)
                | crate::heap::HeapObj::Spread(_),
            )
            | None => Ok(val),
        }
    }

    pub(crate) fn load_module(&mut self, specifier: &str) -> VmResult<VmValue> {
        let source_file = self
            .frames
            .last()
            .map(|f| f.closure().proto.chunk.source_file.clone())
            .unwrap_or_else(|| "".to_owned().into());
        self.load_module_from_source(specifier, source_file.as_ref())
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

        if let Some(&cached) = unsafe { &*self.modules.get() }.get(&resolved) {
            if cached.is_heap() {
                if let Some(HeapObj::FrozenModule(frozen)) = self.heap.get(cached.as_heap()) {
                    let frozen = frozen.clone();
                    let thawed = super::module_freeze::thaw_module(&frozen, &mut self.heap);
                    self.linker.set_done(resolved, thawed);
                    return Ok(thawed);
                }
            }
            return Ok(cached);
        }

        let spec_str = canonical_id_str(&resolved);
        let is_pure = varn_builtins::spec_for(&spec_str).is_some_and(|s| s.pure);
        let builtin_nv = if !is_pure {
            varn_builtins::build_module(&spec_str, &mut self.heap).or_else(|| match &resolved {
                ModuleId::Std(name) | ModuleId::Core(name) | ModuleId::Runtime(name) => {
                    let is_p = varn_builtins::spec_for(name.as_ref()).is_some_and(|s| s.pure);
                    if !is_p {
                        varn_builtins::build_module(name.as_ref(), &mut self.heap)
                    } else {
                        None
                    }
                }
                ModuleId::Local(_) | ModuleId::Package { .. } => None,
            })
        } else {
            None
        };
        if let Some(nv) = builtin_nv {
            let converted = self.convert_to_module_obj(resolved.clone(), nv)?;
            unsafe { &mut *self.modules.get() }.insert(resolved.clone(), converted);
            self.linker.set_done(resolved, converted);
            return Ok(converted);
        }

        if self.linker.is_evaluating(&resolved) {
            return unsafe { &*self.modules.get() }
                .get(&resolved)
                .copied()
                .ok_or_else(|| {
                    RuntimeError::new(format!(
                        "E_BINDING_TDZ: circular dependency on '{specifier}'"
                    ))
                });
        }

        if let Some(proto) = self.precompiled.get(&resolved).cloned() {
            let result = self.eval_module_proto(resolved.clone(), proto);

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
        proto: std::rc::Rc<varn_types::FunctionProto>,
    ) -> VmResult<VmValue> {
        let module_base = self.globals_mut().reserve_region(proto.global_count);

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
        unsafe { &mut *self.modules.get() }.insert(resolved.clone(), module_val);

        self.linker.set_evaluating(resolved.clone());

        let mut closure = crate::exec::calls::build_closure(proto, &mut self.heap, self.settings);
        std::rc::Rc::get_mut(&mut closure)
            .expect("fresh module closure is uniquely owned")
            .module_base = module_base;
        self.push_frame(closure)?;
        let frame_idx = self.frames.len() - 1;
        self.module_exports.insert(frame_idx, module_val);

        let res = match self.run_until(frame_idx) {
            Ok(v) => v,
            Err(e) => {
                self.linker.cancel_evaluating(&resolved);
                unsafe { &mut *self.modules.get() }.remove(&resolved);
                return Err(e);
            }
        };
        if self.vm_suspend.is_some() {
            return Ok(module_val);
        }
        let final_val = unsafe { &*self.modules.get() }
            .get(&resolved)
            .copied()
            .unwrap_or(res);
        unsafe { &mut *self.modules.get() }.insert(resolved.clone(), final_val);

        self.linker.set_done(resolved, final_val);
        Ok(final_val)
    }
}

fn get_cached_exports(module_id: &str) -> Option<Vec<std::sync::Arc<str>>> {
    let spec = varn_builtins::spec_for(module_id)?;
    if spec.exports.is_empty() {
        None
    } else {
        Some(
            spec.exports
                .iter()
                .map(|&s| std::sync::Arc::from(s))
                .collect(),
        )
    }
}
