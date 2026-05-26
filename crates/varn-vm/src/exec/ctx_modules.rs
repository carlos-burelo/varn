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

        // 2. Native modules (std:/core:/runtime: with a Rust implementation).
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
            return self
                .modules
                .get(&resolved)
                .copied()
                .ok_or_else(|| RuntimeError::new(format!("E_BINDING_TDZ: circular dependency on '{specifier}'")));
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

        let res = self.run_until(frame_idx)?;
        if self.vm_suspend.is_some() {
            return Ok(VmValue::null());
        }
        let final_val = self.modules.get(&resolved).copied().unwrap_or(res);
        self.modules.insert(resolved.clone(), final_val);

        // Mark as done in the linker graph.
        self.linker.set_done(resolved, final_val);
        Ok(final_val)
    }
}

fn extract_exports_from_source(source: &str) -> Vec<String> {
    let mut exports = Vec::new();
    let mut brace_depth = 0;
    let mut chars = source.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '/' => {
                if chars.peek() == Some(&'/') {
                    chars.next();
                    while let Some(next_c) = chars.next() {
                        if next_c == '\n' {
                            break;
                        }
                    }
                } else if chars.peek() == Some(&'*') {
                    chars.next();
                    while let Some(next_c) = chars.next() {
                        if next_c == '*' && chars.peek() == Some(&'/') {
                            chars.next();
                            break;
                        }
                    }
                }
            }
            '"' | '\'' | '`' => {
                let quote = c;
                let mut escaped = false;
                while let Some(next_c) = chars.next() {
                    if escaped {
                        escaped = false;
                    } else if next_c == '\\' {
                        escaped = true;
                    } else if next_c == quote {
                        break;
                    }
                }
            }
            '{' => {
                brace_depth += 1;
            }
            '}' => {
                if brace_depth > 0 {
                    brace_depth -= 1;
                }
            }
            _ => {
                if brace_depth == 0 && (c.is_alphabetic() || c == '_') {
                    let mut word = String::new();
                    word.push(c);
                    while let Some(&next_c) = chars.peek() {
                        if next_c.is_alphanumeric() || next_c == '_' {
                            word.push(next_c);
                            chars.next();
                        } else {
                            break;
                        }
                    }

                    if word == "export" {
                        let mut words = Vec::new();
                        while words.len() < 5 {
                            skip_whitespace_and_comments(&mut chars);

                            if let Some(&next_c) = chars.peek() {
                                if next_c.is_alphanumeric() || next_c == '_' {
                                    let mut w = String::new();
                                    while let Some(&nc) = chars.peek() {
                                        if nc.is_alphanumeric() || nc == '_' {
                                            w.push(nc);
                                            chars.next();
                                        } else {
                                            break;
                                        }
                                    }
                                    words.push(w);
                                } else if next_c == '{'
                                    || next_c == '*'
                                    || next_c == '='
                                    || next_c == ';'
                                {
                                    break;
                                } else {
                                    chars.next();
                                    break;
                                }
                            } else {
                                break;
                            }
                        }

                        let mut keyword_idx = 0;
                        if keyword_idx < words.len() && words[keyword_idx] == "declare" {
                            keyword_idx += 1;
                        }

                        if keyword_idx < words.len() {
                            let kw = &words[keyword_idx];
                            if [
                                "function",
                                "class",
                                "interface",
                                "namespace",
                                "type",
                                "enum",
                                "struct",
                                "const",
                                "let",
                            ]
                            .contains(&kw.as_str())
                            {
                                if keyword_idx + 1 < words.len() {
                                    let name = &words[keyword_idx + 1];
                                    exports.push(name.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    exports.sort();
    exports.dedup();
    exports
}

fn skip_whitespace_and_comments(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '/' {
            chars.next();
            if chars.peek() == Some(&'/') {
                chars.next();
                while let Some(next_c) = chars.next() {
                    if next_c == '\n' {
                        break;
                    }
                }
            } else if chars.peek() == Some(&'*') {
                chars.next();
                while let Some(next_c) = chars.next() {
                    if next_c == '*' && chars.peek() == Some(&'/') {
                        chars.next();
                        break;
                    }
                }
            } else {
                break;
            }
        } else {
            break;
        }
    }
}

static STDLIB_EXPORTS_CACHE: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, Vec<std::sync::Arc<str>>>>,
> = std::sync::OnceLock::new();

fn get_cached_exports(module_id: &str) -> Option<Vec<std::rc::Rc<str>>> {
    let cache_mutex = STDLIB_EXPORTS_CACHE
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut cache = cache_mutex.lock().unwrap();
    if let Some(keys) = cache.get(module_id) {
        return Some(keys.iter().map(|k| std::rc::Rc::from(k.as_ref())).collect());
    }

    if let Some(spec) = varn_builtins::spec_for(module_id) {
        if let Some(source) = spec.source() {
            let parsed_exports = extract_exports_from_source(source);
            if !parsed_exports.is_empty() {
                let cache_keys: Vec<std::sync::Arc<str>> = parsed_exports
                    .iter()
                    .map(|k| std::sync::Arc::from(k.as_str()))
                    .collect();
                let keys: Vec<std::rc::Rc<str>> = parsed_exports
                    .into_iter()
                    .map(|k| std::rc::Rc::from(k.as_str()))
                    .collect();
                cache.insert(module_id.to_string(), cache_keys);
                return Some(keys);
            }
        }
    }
    None
}
