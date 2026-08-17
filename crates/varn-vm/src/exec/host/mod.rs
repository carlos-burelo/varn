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

use super::calls::PreparedCall;
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

    fn map_key(&mut self, v: VmValue) -> varn_types::value::MapKey {
        self.heap.canonical_map_key(v)
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

            if let Some(HeapObj::Module(m)) = self.heap.get(obj.as_heap_idx()) {
                let slot = m.export_map.get(key).copied()?;
                return m.get_slot(slot);
            }
        }
        None
    }

    fn set_field(&mut self, obj: VmValue, key: &str, val: VmValue) {
        if obj.is_heap() {
            if let Some(HeapObj::Object(o)) = self.heap.get(obj.as_heap_idx()) {
                o.set_field_nv(std::rc::Rc::from(key), val);
                self.heap.write_barrier(obj.as_heap_idx(), val);
            } else if let Some(HeapObj::Module(m)) = self.heap.get_mut(obj.as_heap_idx()) {
                if let Some(s) = m.export_map.get(key).copied() {
                    std::rc::Rc::make_mut(m).set_slot(s, val);
                } else {
                    let m = std::rc::Rc::make_mut(m);
                    let slot = m.exports.len();
                    m.exports.push(val);
                    m.export_map.insert(std::rc::Rc::from(key), slot);
                }
            }
        }
    }

    fn alloc_fn(&mut self, f: NativeFn, name: &'static str) -> VmValue {
        self.heap.alloc_native_fn(f, name)
    }

    fn alloc_class(&mut self, class: std::rc::Rc<ClassObj>) -> VmValue {
        self.heap.intern(varn_types::Value::Class(class))
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

    fn call_vm(&mut self, callee: VmValue, args: &[VmValue]) -> Result<VmValue, String> {
        let orig_len = self.stack.len();
        self.stack.push(callee);
        self.stack.extend_from_slice(args);
        let prepared = match self.prepare_call(callee, args.len() + 1) {
            Ok(p) => p,
            Err(e) => {
                self.stack.truncate(orig_len);
                return Err(e.message);
            }
        };
        let res = match prepared {
            PreparedCall::NativeImmediate(f, arg_count) => {
                let args_start = self.stack.len() - arg_count;
                let vm_args: Vec<VmValue> = self.stack[args_start..args_start + arg_count].to_vec();
                (f)(self as &mut dyn NativeCtx, &vm_args)
            }
            PreparedCall::RawNativeImmediate(f, arg_count) => {
                let args_start = self.stack.len() - arg_count;
                let vm_args: Vec<VmValue> = self.stack[args_start..args_start + arg_count].to_vec();
                let slice = if arg_count > 0 {
                    &vm_args[1..]
                } else {
                    &vm_args[..]
                };
                (f)(self as &mut dyn NativeCtx, slice)
            }
            PreparedCall::Frame(frame) => {
                let depth = self.frames.len();
                let required = frame.base + frame.closure().proto.register_count as usize;
                if self.stack.len() < required {
                    self.stack.resize(required, VmValue::null());
                }
                self.frames.push(frame);
                self.run_until(depth).map_err(|e| e.message)
            }
            PreparedCall::PushValue(nv) => Ok(nv),
            PreparedCall::Generator {
                closure,
                args,
                current_class,
            } => Ok(self.build_generator(closure, args, current_class)),
            PreparedCall::Constructor(frame, instance_nv) => {
                let depth = self.frames.len();
                let required = frame.base + frame.closure().proto.register_count as usize;
                if self.stack.len() < required {
                    self.stack.resize(required, VmValue::null());
                }
                self.frames.push(frame);
                self.pending_constructors.push((depth, instance_nv));
                let _ = self.run_until(depth).map_err(|e| e.message)?;
                Ok(instance_nv)
            }
            PreparedCall::NativeConstructor(f, args, instance_nv) => {
                let result = (f)(self as &mut dyn NativeCtx, &args).map_err(|e| e)?;
                let nv = if result.is_null() {
                    instance_nv
                } else {
                    result
                };
                Ok(nv)
            }
        };
        self.stack.truncate(orig_len);
        res
    }

    fn spawn_vm(&mut self, callee: VmValue, args: &[VmValue]) -> Result<VmValue, String> {
        let value = self.heap.extract(callee);
        let val_args: Vec<varn_types::Value> = args.iter().map(|&a| self.heap.extract(a)).collect();
        let task = self.spawn_internal(value, &val_args).map_err(|e| e)?;
        Ok(self.heap.intern(task))
    }

    /// Timers block the calling thread.
    ///
    /// There is no event loop to hand them to: tasks run on a synchronous
    /// trampoline inside the VM (docs/RUNTIME_ARCHITECTURE.md §1), so either
    /// this sleeps or nothing ever resolves the handle.
    fn suspend_timer(&mut self, ms: u64) -> VmValue {
        let output = varn_types::AsyncTask::pending();
        std::thread::sleep(std::time::Duration::from_millis(ms));
        output.resolve(varn_types::Value::Null);
        self.heap.intern(varn_types::Value::TaskHandle(output))
    }

    fn resources(&mut self) -> &mut ResourceStore {
        &mut self.resources
    }

    fn extract(&self, v: VmValue) -> varn_types::Value {
        self.heap.extract(v)
    }

    fn intern(&mut self, v: varn_types::Value) -> VmValue {
        self.heap.intern(v)
    }

    fn call_static(&mut self, f: NativeFn) -> VmValue {
        varn_types::call_static_with(self, f)
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
    ) -> Result<varn_types::AsyncTask, String> {
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
                        if let Some(wrapper) = closure
                            .as_any()
                            .downcast_ref::<crate::closure::VmClosurePayload>()
                        {
                            let c = &wrapper.0;
                            let source_file = c.proto.chunk.source_file.to_string();
                            let name = c.proto.name.as_ref()?.to_string();
                            Some((source_file, name))
                        } else {
                            None
                        }
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
}
