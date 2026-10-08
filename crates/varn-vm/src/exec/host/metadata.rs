use super::*;

impl ExecCtx {
    pub(super) fn host_str_owned(&self, v: VmValue) -> Option<String> {
        self.heap.str_owned(v)
    }
    pub(super) fn host_current_source_file(&self) -> Option<String> {
        for frame in self.frames.iter().rev() {
            let src = &frame.closure().proto.chunk.source_file;
            if !src.starts_with("std:") && !src.starts_with("runtime:") && !src.starts_with("core:")
            {
                return Some(src.to_string());
            }
        }
        self.frames
            .last()
            .map(|f| f.closure().proto.chunk.source_file.to_string())
    }
    pub(super) fn host_get_function_location(&self, func_val: VmValue) -> Option<(String, String)> {
        if func_val.is_heap() {
            match self.heap.get(func_val.as_heap()) {
                Some(HeapObj::VmClosure(c)) => {
                    let source_file = c.proto.chunk.source_file.to_string();
                    let name = c.proto.name.as_ref()?.to_string();
                    Some((source_file, name))
                }
                Some(HeapObj::BoundMethod(bm)) => match &bm.target {
                    varn_types::value::BoundMethodTarget::Vm { closure, .. } => {
                        let c = self.heap.closure_of(*closure)?;
                        let source_file = c.proto.chunk.source_file.to_string();
                        let name = c.proto.name.as_ref()?.to_string();
                        Some((source_file, name))
                    }
                    varn_types::value::BoundMethodTarget::Native { .. } => None,
                },
                Some(HeapObj::Str(_) | HeapObj::Array(_) | HeapObj::Tuple(_) | HeapObj::Object(_) | HeapObj::Record(_) | HeapObj::Buffer(_) | HeapObj::Module(_) | HeapObj::FrozenModule(_) | HeapObj::Class(_) | HeapObj::NativeFn(..) | HeapObj::Map(_) | HeapObj::Set(_) | HeapObj::Task(_) | HeapObj::TaskHandle(_) | HeapObj::Range(_) | HeapObj::Symbol(_) | HeapObj::EnumVariant(_) | HeapObj::BigInt(_) | HeapObj::Decimal(_) | HeapObj::Char(_) | HeapObj::Generator(_) | HeapObj::Spread(_)) | None => None,
            }
        } else {
            None
        }
    }
    pub(super) fn host_parse_csv(
        &mut self,
        text: &str,
        delimiter: u8,
        has_header: bool,
        trim: bool,
    ) -> Result<VmValue, String> {
        crate::exec::ctx_csv::parse_csv(self, text, delimiter, has_header, trim)
    }
    pub(super) fn host_define_metadata(&mut self, target: VmValue, key: &str, value: VmValue) {
        let target_k = self.target_meta_key(target);
        unsafe { &mut *self.metadata.get() }
            .entry(target_k)
            .or_default()
            .insert(key.to_string(), value);
    }
    pub(super) fn host_get_metadata(&self, target: VmValue, key: &str) -> Option<VmValue> {
        let target_k = self.target_meta_key(target);
        unsafe { &*self.metadata.get() }
            .get(&target_k)
            .and_then(|m| m.get(key))
            .copied()
    }
    pub(super) fn host_has_metadata(&self, target: VmValue, key: &str) -> bool {
        let target_k = self.target_meta_key(target);
        unsafe { &*self.metadata.get() }
            .get(&target_k)
            .map(|m| m.contains_key(key))
            .unwrap_or(false)
    }
}

impl ExecCtx {
    pub(crate) fn target_meta_key(&self, v: VmValue) -> String {
        if v.is_heap() {
            if let Some(obj) = self.heap.get(v.as_heap()) {
                match obj {
                    HeapObj::Class(ref cls) => format!("class:{:p}", std::rc::Rc::as_ptr(cls)),
                    HeapObj::VmClosure(ref c) => {
                        format!("fn:{:p}", std::rc::Rc::as_ptr(&c.proto))
                    }
                    HeapObj::Object(ref oref) => {
                        format!("obj:{:p}", oref.read() as *const _ as *const u8)
                    }
                    HeapObj::Str(_) | HeapObj::Array(_) | HeapObj::Tuple(_) | HeapObj::Record(_) | HeapObj::Buffer(_) | HeapObj::Module(_) | HeapObj::FrozenModule(_) | HeapObj::NativeFn(..) | HeapObj::BoundMethod(_) | HeapObj::Map(_) | HeapObj::Set(_) | HeapObj::Task(_) | HeapObj::TaskHandle(_) | HeapObj::Range(_) | HeapObj::Symbol(_) | HeapObj::EnumVariant(_) | HeapObj::BigInt(_) | HeapObj::Decimal(_) | HeapObj::Char(_) | HeapObj::Generator(_) | HeapObj::Spread(_) => format!("heap:{:x}", v.as_heap().addr()),
                }
            } else {
                format!("heap:{:x}", v.as_heap().addr())
            }
        } else if v.is_int() {
            format!("int:{}", v.as_int())
        } else {
            self.heap.str_repr(v)
        }
    }
}
