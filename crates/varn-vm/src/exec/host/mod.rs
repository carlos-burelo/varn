//! The host boundary: how the VM answers `NativeCtx`.
//!
//! Everything a native builtin can ask of the running program — allocate a
//! string, read a field, call back into VM code, spawn an isolate — arrives
//! through this one trait impl. It lived inside `frame_ctrl` for no reason
//! other than that both happen to be written against `ExecCtx`; call/return
//! sequencing and the host ABI are not the same domain, and the file was
//! over 1000 lines because they shared it.
//!
//! Cross-isolate value transfer and the VM-call window live in [`isolates`].

pub(crate) mod isolates;

use super::ctx::ExecCtx;
use crate::heap::HeapObj;
use crate::value::VmValue;
use varn_types::{ClassObj, NativeCtx, NativeFn, ResourceStore};

impl NativeCtx for ExecCtx {
    // Native results are runtime-produced values: allocate without interning
    // (`alloc_str` would hash the full contents and retain a reference on the
    // old-gen path — see `alloc_str_dynamic`'s contract).
    fn alloc_str(&mut self, s: &str) -> VmValue {
        self.heap.alloc_str_dynamic(s)
    }

    fn map_key(
        &mut self,
        v: VmValue,
    ) -> Result<varn_types::value::MapKey, varn_types::NativeError> {
        let v = self.hashable_key(v)?;
        Ok(self.heap.canonical_map_key(v))
    }

    // Map keys MUST canonicalize through the content interner —
    // `alloc_str_dynamic` (and the trait default's `intern`) would mint a
    // fresh index per call and break key equality.
    fn str_map_key(&mut self, s: &str) -> varn_types::value::MapKey {
        match VmValue::try_from_sso(s) {
            Some(v) => varn_types::value::MapKey(v),
            None => varn_types::value::MapKey(self.heap.alloc_str_interned(s)),
        }
    }

    fn collection_write_barrier(&mut self, parent: VmValue, child: VmValue) {
        if parent.is_heap() {
            self.heap.write_barrier(parent.as_heap_idx(), child);
        }
    }

    fn alloc_str_owned(&mut self, s: String) -> VmValue {
        self.heap.alloc_str_dynamic(&s)
    }

    fn str_repr(&self, v: VmValue) -> String {
        self.heap.str_repr(v)
    }

    fn str_repr_borrowed<'a>(&'a self, v: VmValue) -> std::borrow::Cow<'a, str> {
        self.heap.str_repr_borrowed(v)
    }

    fn str_owned(&self, v: VmValue) -> Option<String> {
        self.heap.str_owned(v)
    }

    // Both delegate to `Heap`'s `NativeCtx` impl — its `str_shared` is a
    // refcount bump (no copy) and `str_is_ascii` reads `HeapStr`'s cached
    // flag (O(1) amortized). Missing these here meant every native method
    // called through `ExecCtx` (which is every `CallNativeOp` site) fell
    // through to the trait's defaults instead: `str_shared` copying the
    // whole string per call, `str_is_ascii` re-scanning it per call — an
    // O(n) cost repeated on every element of a sequential scan, i.e. the
    // very thing the cached flag exists to avoid.
    fn str_shared(&self, v: VmValue) -> Option<std::sync::Arc<str>> {
        self.heap.str_shared(v)
    }

    fn str_is_ascii(&self, v: VmValue) -> bool {
        self.heap.str_is_ascii(v)
    }

    fn is_string(&self, v: VmValue) -> bool {
        v.is_sso()
            || (v.is_heap() && matches!(self.heap.get(v.as_heap_idx()), Some(HeapObj::Str(_))))
    }

    fn is_array(&self, v: VmValue) -> bool {
        v.is_heap()
            && matches!(
                self.heap.get(v.as_heap_idx()),
                Some(HeapObj::Array(_) | HeapObj::Tuple(_))
            )
    }

    fn alloc_array(&mut self, items: Vec<VmValue>) -> VmValue {
        self.heap.alloc_array_vm(items)
    }

    fn array_len(&self, arr: VmValue) -> usize {
        crate::heap_array::array_len(&self.heap, arr)
    }

    fn array_get(&self, arr: VmValue, idx: usize) -> Option<VmValue> {
        crate::heap_array::array_get(&self.heap, arr, idx)
    }

    fn array_set(&mut self, arr: VmValue, idx: usize, val: VmValue) {
        crate::heap_array::array_set(&mut self.heap, arr, idx, val)
    }

    fn array_push(&mut self, arr: VmValue, val: VmValue) {
        crate::heap_array::array_push(&mut self.heap, arr, val)
    }

    fn array_pop(&mut self, arr: VmValue) -> Option<VmValue> {
        crate::heap_array::array_pop(&self.heap, arr)
    }

    fn array_for_each(&self, arr: VmValue, f: &mut dyn FnMut(VmValue, usize)) {
        crate::heap_array::array_for_each(&self.heap, arr, f)
    }

    fn is_object(&self, v: VmValue) -> bool {
        if v.is_heap() {
            matches!(
                self.heap.get(v.as_heap_idx()),
                Some(HeapObj::Object(_) | HeapObj::Record(_))
            )
        } else {
            false
        }
    }

    fn object_for_each(&self, obj: VmValue, f: &mut dyn FnMut(&str, VmValue)) {
        if obj.is_heap() {
            if let Some(HeapObj::Object(o) | HeapObj::Record(o)) = self.heap.get(obj.as_heap_idx())
            {
                for (k, v) in o.borrow().iter() {
                    f(k.as_ref(), v);
                }
            }
        }
    }

    fn map_for_each(&self, map: VmValue, f: &mut dyn FnMut(VmValue, VmValue)) {
        if map.is_heap() {
            if let Some(HeapObj::Map(m)) = self.heap.get(map.as_heap_idx()) {
                for (k, v) in m.0.borrow().iter() {
                    f(k.0, *v);
                }
            }
        }
    }

    fn get_object_shape(&self, obj: VmValue) -> Option<std::rc::Rc<varn_types::Shape>> {
        if obj.is_heap() {
            if let Some(HeapObj::Object(o) | HeapObj::Record(o)) = self.heap.get(obj.as_heap_idx())
            {
                return Some(std::rc::Rc::clone(o.borrow().shape()));
            }
        }
        None
    }

    fn alloc_object(&mut self) -> VmValue {
        self.heap.alloc_object()
    }

    fn alloc_object_with_shape(
        &mut self,
        shape: &std::rc::Rc<varn_types::Shape>,
        values: Vec<VmValue>,
    ) -> VmValue {
        self.heap.alloc_object_with_shape(shape, values)
    }

    fn get_field(&self, obj: VmValue, key: &str) -> Option<VmValue> {
        if obj.is_heap() {
            if let Some(HeapObj::Object(o) | HeapObj::Record(o)) = self.heap.get(obj.as_heap_idx())
            {
                return o.borrow().get_field_nv(key);
            }

            if let Some(HeapObj::Instance(inst)) = self.heap.get(obj.as_heap_idx()) {
                let cls = ClassObj::find_by_id(inst.class_id)?;
                let layout = cls.get_or_compute_layout();
                let f = layout.get_field(key)?;
                return inst.read_field(f);
            }

            if let Some(HeapObj::Module(m)) = self.heap.get(obj.as_heap_idx()) {
                let slot = m.export_map.get(key).copied()?;
                return m.get_slot(slot);
            }
        }
        None
    }

    fn set_field(&mut self, obj: VmValue, key: &str, val: VmValue) {
        if obj.is_heap() {
            let idx = obj.as_heap_idx();
            if let Some(HeapObj::Object(o)) = self.heap.get(idx) {
                o.set_field_nv(std::sync::Arc::from(key), val);
                self.heap.write_barrier(idx, val);
            } else if let Some(HeapObj::Instance(inst)) = self.heap.get(idx) {
                let inst = inst.clone();
                let field = ClassObj::find_by_id(inst.class_id)
                    .map(|cls| cls.get_or_compute_layout())
                    .and_then(|layout| layout.get_field(key).cloned());
                if let Some(f) = field {
                    if inst.write_field(&f, val).is_ok() {
                        self.heap.write_barrier(idx, val);
                    }
                }
            } else if let Some(HeapObj::Module(m)) = self.heap.get_mut(idx) {
                if let Some(s) = m.export_map.get(key).copied() {
                    std::rc::Rc::make_mut(m).set_slot(s, val);
                } else {
                    let m = std::rc::Rc::make_mut(m);
                    let slot = m.exports.len();
                    m.exports.push(val);
                    m.export_map.insert(std::sync::Arc::from(key), slot);
                }
            }
        }
    }

    fn alloc_fn(&mut self, f: NativeFn, name: &'static str) -> VmValue {
        self.heap.alloc_native_fn(f, name)
    }

    fn alloc_class(&mut self, class: std::rc::Rc<ClassObj>) -> VmValue {
        self.heap.alloc_class_vm(class)
    }

    fn alloc_range(&mut self, start: i64, end: i64, inclusive: bool) -> VmValue {
        self.heap.alloc_range(start, end, inclusive)
    }

    fn alloc_buffer(&mut self, size: usize) -> VmValue {
        self.heap.alloc_vm_buffer(varn_types::VmBuffer::new(size))
    }

    fn alloc_buffer_from_bytes(&mut self, bytes: &[u8]) -> VmValue {
        self.heap
            .alloc_vm_buffer(varn_types::VmBuffer::from_bytes(bytes))
    }

    fn is_buffer(&self, v: VmValue) -> bool {
        if v.is_heap() {
            matches!(self.heap.get(v.as_heap_idx()), Some(HeapObj::Buffer(_)))
        } else {
            false
        }
    }

    fn buffer_len(&self, v: VmValue) -> usize {
        if v.is_heap() {
            if let Some(HeapObj::Buffer(b)) = self.heap.get(v.as_heap_idx()) {
                return b.len();
            }
        }
        0
    }

    fn buffer_get_byte(&self, v: VmValue, idx: usize) -> Option<u8> {
        if v.is_heap() {
            if let Some(HeapObj::Buffer(b)) = self.heap.get(v.as_heap_idx()) {
                return b.as_slice().get(idx).copied();
            }
        }
        None
    }

    fn buffer_set_byte(&mut self, v: VmValue, idx: usize, byte: u8) -> bool {
        if v.is_heap() {
            if let Some(HeapObj::Buffer(b)) = self.heap.get_mut(v.as_heap_idx()) {
                let mut slice = b.as_mut_slice();
                if idx < slice.len() {
                    slice[idx] = byte;
                    return true;
                }
            }
        }
        false
    }

    fn buffer_slice(&mut self, v: VmValue, start: usize, end: usize) -> Option<VmValue> {
        if v.is_heap() {
            if let Some(HeapObj::Buffer(b)) = self.heap.get(v.as_heap_idx()) {
                let sub = b.slice(start, end);
                return Some(self.heap.alloc_vm_buffer(sub));
            }
        }
        None
    }

    fn buffer_to_string(&self, v: VmValue) -> Option<String> {
        if v.is_heap() {
            if let Some(HeapObj::Buffer(b)) = self.heap.get(v.as_heap_idx()) {
                let slice = b.as_slice();
                return String::from_utf8(slice.to_vec()).ok();
            }
        }
        None
    }

    fn buffer_to_bytes(&self, v: VmValue) -> Option<Vec<u8>> {
        if v.is_heap() {
            if let Some(HeapObj::Buffer(b)) = self.heap.get(v.as_heap_idx()) {
                return Some(b.as_slice().to_vec());
            }
        }
        None
    }

    fn call_vm(
        &mut self,
        callee: VmValue,
        args: &[VmValue],
    ) -> Result<VmValue, varn_types::NativeError> {
        // The window is `[callee, args...]`, the exact shape the interpreter's
        // callee slot + arguments and the compiled caller's flushed staging
        // produce; `invoke` is the single run-to-completion entry.
        let mut window = Vec::with_capacity(args.len() + 1);
        window.push(callee);
        window.extend_from_slice(args);
        Ok(self.invoke(callee, &window)?)
    }

    fn method(&mut self, recv: VmValue, name: &str) -> Option<VmValue> {
        self.bound_method(recv, name)
    }

    fn spawn_vm(&mut self, callee: VmValue, args: &[VmValue]) -> Result<VmValue, String> {
        self.spawn_internal(callee, args)
    }

    fn suspend_timer(&mut self, ms: u64) -> VmValue {
        varn_builtins::modules::net::driver::driver();
        let promise = varn_runtime::timer::sleep_task(ms);
        self.task_from_host(promise, varn_types::HostOpen::Plain)
    }

    fn task_new(&mut self) -> VmValue {
        crate::task::alloc_handle(&mut self.heap, crate::task::TaskCell::pending())
    }

    fn task_resolved(&mut self, value: VmValue) -> VmValue {
        let cell = crate::task::TaskCell::pending();
        crate::task::settle(&mut self.heap, &cell, Ok(value));
        crate::task::alloc_handle(&mut self.heap, cell)
    }

    fn task_rejected(&mut self, value: VmValue) -> VmValue {
        let cell = crate::task::TaskCell::pending();
        crate::task::settle(&mut self.heap, &cell, Err(value));
        crate::task::alloc_handle(&mut self.heap, cell)
    }

    fn task_from_host(
        &mut self,
        promise: varn_types::HostPromise,
        open: varn_types::HostOpen,
    ) -> VmValue {
        let cell = crate::task::TaskCell::host(promise.clone(), open);
        let handle = crate::task::alloc_handle(&mut self.heap, std::rc::Rc::clone(&cell));
        crate::exec::scheduler::adopt_host(cell, &promise);
        handle
    }

    fn alloc_bigint(&mut self, value: num_bigint::BigInt) -> VmValue {
        self.heap.alloc_bigint(value)
    }

    fn alloc_decimal(&mut self, value: bigdecimal::BigDecimal) -> VmValue {
        self.heap.alloc_decimal(value)
    }

    fn alloc_char(&mut self, value: char) -> VmValue {
        self.heap.alloc_char(value)
    }

    fn alloc_map(&mut self, entries: Vec<(VmValue, VmValue)>) -> VmValue {
        let mut map = varn_types::value::ValueMap::default();
        for (key, value) in entries {
            let key = self.map_key(key).unwrap_or(varn_types::value::MapKey(key));
            map.insert(key, value);
        }
        self.heap.alloc_map_vm(map)
    }

    fn alloc_set(&mut self, items: Vec<VmValue>) -> VmValue {
        let mut set = varn_types::value::ValueSet::default();
        for item in items {
            let key = self
                .map_key(item)
                .unwrap_or(varn_types::value::MapKey(item));
            set.insert(key);
        }
        self.heap.alloc_set_vm(set)
    }

    fn alloc_enum_variant(&mut self, data: varn_types::value::EnumVariantData) -> VmValue {
        self.heap.alloc_enum_variant_vm(data)
    }

    fn alloc_bound_native(
        &mut self,
        receiver: VmValue,
        func: NativeFn,
        name: &'static str,
    ) -> VmValue {
        self.heap.alloc_bound_native(receiver, func, name)
    }

    fn task_gather(&mut self, tasks: VmValue) -> Result<VmValue, String> {
        isolates::gather_tasks(self, tasks)
    }

    fn task_yield(&mut self) -> VmValue {
        crate::task::alloc_handle(&mut self.heap, crate::task::TaskCell::yielded())
    }

    fn task_cancel(&mut self, task: VmValue) -> Result<(), String> {
        let cell = match self.task_cell(task) {
            Some(cell) => cell,
            None => return Err("cancel: expected a task handle".to_string()),
        };
        if cell.is_yield() {
            return Ok(());
        }
        if let Some(promise) = cell.host_promise() {
            promise.reject_msg("Task cancelled");
        }
        let reason = self.heap.alloc_str("Task cancelled");
        crate::task::settle(&mut self.heap, &cell, Err(reason));
        Ok(())
    }

    /// `&mut` out of the shared table under the same single-threaded
    /// contract as the heap: tasks on this thread take turns, never
    /// overlapping, so no aliasing `&mut` exists at once.
    #[allow(clippy::mut_from_ref)]
    fn resources(&mut self) -> &mut ResourceStore {
        unsafe { &mut *self.resources.get() }
    }

    fn as_generator(&self, v: VmValue) -> Option<varn_types::GeneratorObj> {
        self.heap.generator_of(v)
    }

    fn as_char(&self, v: VmValue) -> Option<char> {
        self.heap.char_of(v)
    }

    fn as_bigint(&self, v: VmValue) -> Option<num_bigint::BigInt> {
        self.heap.bigint_of(v)
    }

    fn as_decimal(&self, v: VmValue) -> Option<bigdecimal::BigDecimal> {
        self.heap.decimal_of(v)
    }

    fn as_range(&self, v: VmValue) -> Option<varn_types::value::RangeData> {
        self.heap.range_of(v)
    }

    fn as_map(&self, v: VmValue) -> Option<varn_types::value::MapRef> {
        self.heap.map_of(v)
    }

    fn as_set(&self, v: VmValue) -> Option<varn_types::value::SetRef> {
        self.heap.set_of(v)
    }

    fn is_static_receiver(&self, v: VmValue) -> bool {
        self.heap.is_static_receiver(v)
    }

    fn alloc_range_data(&mut self, range: varn_types::value::RangeData) -> VmValue {
        self.heap.alloc_range_data(range)
    }

    fn call_static(&mut self, f: NativeFn) -> VmValue {
        varn_types::call_static_with(f)
    }

    fn get_class(&self, name: &str) -> Option<std::rc::Rc<ClassObj>> {
        self.heap.get_intrinsic_class(name)
    }

    fn register_class(&mut self, name: &str, cls: std::rc::Rc<ClassObj>) {
        self.heap.set_intrinsic_class(name, cls);
    }

    fn current_source_file(&self) -> Option<String> {
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

    fn spawn_isolate(
        &mut self,
        module_path: &str,
        export_name: &str,
        args: Vec<varn_types::value::SendValue>,
    ) -> Result<varn_types::HostPromise, String> {
        isolates::spawn_isolate(self, module_path, export_name, args)
    }

    fn alloc_instance(&mut self, class_name: &str) -> Option<VmValue> {
        let class_obj = self.get_class(class_name)?;
        let instance_nv = self.heap.alloc_object();
        if let Some(crate::heap::HeapObj::Object(o)) = self.heap.get_mut(instance_nv.as_heap_idx())
        {
            o.set_class(class_obj);
        }
        Some(instance_nv)
    }

    fn get_function_location(&self, func_val: VmValue) -> Option<(String, String)> {
        if func_val.is_heap() {
            match self.heap.get_by_idx(func_val.as_heap_idx()) {
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
                    _ => None,
                },
                _ => None,
            }
        } else {
            None
        }
    }

    fn load_module(&mut self, specifier: &str) -> Result<VmValue, String> {
        self.load_module(specifier).map_err(|e| format!("{:?}", e))
    }

    fn to_sendable(&self, val: VmValue) -> Result<varn_types::value::SendValue, String> {
        isolates::to_sendable(self, val)
    }

    fn parse_json(&mut self, text: &str) -> Result<VmValue, String> {
        self.json_parse(text)
    }

    fn stringify_json(&mut self, value: VmValue) -> Result<String, String> {
        self.json_stringify(value)
    }

    fn parse_csv(
        &mut self,
        text: &str,
        delimiter: u8,
        has_header: bool,
        trim: bool,
    ) -> Result<VmValue, String> {
        crate::exec::ctx_csv::parse_csv(self, text, delimiter, has_header, trim)
    }

    fn stringify_csv(&mut self, value: VmValue, delimiter: u8) -> Result<String, String> {
        crate::exec::ctx_csv::stringify_csv(self, value, delimiter)
    }

    fn capabilities(&self) -> &varn_types::capabilities::CapabilitySet {
        &self.capabilities
    }

    fn define_metadata(&mut self, target: VmValue, key: &str, value: VmValue) {
        let target_k = self.target_meta_key(target);
        unsafe { &mut *self.metadata.get() }
            .entry(target_k)
            .or_default()
            .insert(key.to_string(), value);
    }

    fn get_metadata(&self, target: VmValue, key: &str) -> Option<VmValue> {
        let target_k = self.target_meta_key(target);
        unsafe { &*self.metadata.get() }
            .get(&target_k)
            .and_then(|m| m.get(key))
            .copied()
    }

    fn has_metadata(&self, target: VmValue, key: &str) -> bool {
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
            if let Some(obj) = self.heap.get(v.as_heap_idx()) {
                match obj {
                    HeapObj::Class(ref cls) => format!("class:{:p}", std::rc::Rc::as_ptr(cls)),
                    HeapObj::VmClosure(ref c) => {
                        format!("fn:{:p}", std::rc::Rc::as_ptr(&c.proto))
                    }
                    HeapObj::Object(ref oref) => {
                        format!("obj:{:p}", std::rc::Rc::as_ptr(&oref.0))
                    }
                    _ => format!("heap:{:x}", v.as_heap_idx()),
                }
            } else {
                format!("heap:{:x}", v.as_heap_idx())
            }
        } else if v.is_int() {
            format!("int:{}", v.as_int())
        } else {
            self.heap.str_repr(v)
        }
    }
}
