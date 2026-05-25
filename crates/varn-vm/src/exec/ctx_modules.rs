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

                let expected_keys = get_cached_exports(&id.as_str());

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
        if let Some(&cached) = self.modules.get(&resolved.as_str()) {
            return Ok(cached);
        }

        if let ModuleId::Stdlib(ref name) = resolved {
            if let Some(nv) = varn_builtins::build_module(name, &mut self.heap) {
                let converted = self.convert_to_module_obj(resolved.clone(), nv)?;
                self.modules
                    .insert(resolved.as_str().to_string(), converted);
                return Ok(converted);
            }
        }

        if let Some(proto) = self.precompiled.get(&resolved.as_str()).cloned() {
            if let Some(existing_idx) = self.frames.iter().position(|f| {
                f.closure
                    .proto
                    .chunk
                    .module_id
                    .as_ref()
                    .map(|id| id.as_str() == resolved.as_str())
                    .unwrap_or(false)
            }) {
                let res = self.run_until(existing_idx)?;
                if self.vm_suspend.is_some() {
                    return Ok(VmValue::null());
                }
                let final_val = self.modules.get(&resolved.as_str()).copied().unwrap_or(res);
                self.modules.insert(resolved.as_str(), final_val);
                return Ok(final_val);
            }

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
            self.modules
                .insert(resolved.as_str().to_string(), module_val);

            let closure = crate::exec::calls::build_closure(proto, &mut self.heap);
            self.push_frame(closure)?;
            let frame_idx = self.frames.len() - 1;
            self.module_exports.insert(frame_idx, module_val);

            let res = self.run_until(frame_idx)?;
            if self.vm_suspend.is_some() {
                return Ok(VmValue::null());
            }
            let final_val = self.modules.get(&resolved.as_str()).copied().unwrap_or(res);
            self.modules.insert(resolved.as_str(), final_val);
            return Ok(final_val);
        }

        if let Some(loader) = &self.loader {
            if let Ok(Some(proto)) = loader.load(&resolved) {
                if let Some(existing_idx) = self.frames.iter().position(|f| {
                    f.closure
                        .proto
                        .chunk
                        .module_id
                        .as_ref()
                        .map(|id| id.as_str() == resolved.as_str())
                        .unwrap_or(false)
                }) {
                    let res = self.run_until(existing_idx)?;
                    if self.vm_suspend.is_some() {
                        return Ok(VmValue::null());
                    }
                    let final_val = self.modules.get(&resolved.as_str()).copied().unwrap_or(res);
                    self.modules.insert(resolved.as_str(), final_val);
                    return Ok(final_val);
                }

                let mut export_map = rustc_hash::FxHashMap::default();
                for (idx, name) in proto.export_names.iter().enumerate() {
                    export_map.insert(name.clone(), idx);
                }
                let mut module_obj = ModuleObj::new(resolved.clone(), proto.export_names.len());
                module_obj.export_map = export_map;
                let module_val = self.heap.alloc_module(std::rc::Rc::new(module_obj));
                self.modules
                    .insert(resolved.as_str().to_string(), module_val);

                let closure = crate::exec::calls::build_closure(proto, &mut self.heap);
                self.push_frame(closure)?;
                let frame_idx = self.frames.len() - 1;
                self.module_exports.insert(frame_idx, module_val);

                let res = self.run_until(frame_idx)?;
                if self.vm_suspend.is_some() {
                    return Ok(VmValue::null());
                }
                let final_val = self.modules.get(&resolved.as_str()).copied().unwrap_or(res);
                self.modules.insert(resolved.as_str(), final_val);
                return Ok(final_val);
            }
        }

        Err(RuntimeError::new(format!(
            "module not found: {}",
            specifier
        )))
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
                    // Line comment, skip to end of line
                    chars.next();
                    while let Some(next_c) = chars.next() {
                        if next_c == '\n' {
                            break;
                        }
                    }
                } else if chars.peek() == Some(&'*') {
                    // Block comment, skip until */
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
                // String literal, skip until matching quote
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
                // If we are at top-level (brace_depth == 0) and we see a word character, let's see if it's "export"
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
                        // We found an export declaration at the top level!
                        // Now let's skip whitespace/comments until we find the identifier
                        let mut words = Vec::new();
                        while words.len() < 5 {
                            // Skip whitespace and comments
                            skip_whitespace_and_comments(&mut chars);

                            // Peek the next character. If it's a word character, read the word.
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
            chars.next(); // Consume '/'
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
