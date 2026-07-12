use super::calls::PreparedCall;
use super::ctx::ExecCtx;
use crate::error::{RuntimeError, VmResult};
use crate::heap::HeapObj;
use crate::value::VmValue;
use varn_types::{ClassObj, NativeCtx, NativeFn, ResourceStore};

impl ExecCtx {
    pub(crate) fn dispatch_prepared_call(&mut self, call: PreparedCall) -> VmResult<()> {
        match call {
            PreparedCall::Frame(frame) => {
                if self.frames.len() >= 10000 {
                    return Err(RuntimeError::new(
                        "stack overflow: call depth exceeded 10000",
                    ));
                }
                self.record_call_vm_fast();
                let required = frame.base + frame.closure().proto.register_count as usize;
                if self.stack.len() < required {
                    self.stack.resize(required, VmValue::null());
                }
                self.record_frame_push();
                self.frames.push(frame);

                if self.heap.needs_minor_gc() {
                    self.run_minor_gc();
                }

                if self.heap.needs_gc() {
                    let mut roots: Vec<u32> = Vec::with_capacity(256);
                    for v in &self.stack {
                        if v.is_heap() {
                            roots.push(v.as_heap_idx());
                        }
                    }
                    for v in &self.globals.values {
                        if v.is_heap() {
                            roots.push(v.as_heap_idx());
                        }
                    }

                    for frame in &self.frames {
                        for c in frame.closure().constants.iter() {
                            if c.is_heap() {
                                roots.push(c.as_heap_idx());
                            }
                        }
                    }
                    for (_k, v) in &self.modules {
                        if v.is_heap() {
                            roots.push(v.as_heap_idx());
                        }
                    }
                    let _ = self.heap.collect(&roots);
                }
            }
            PreparedCall::Constructor(frame, instance_nv) => {
                if self.frames.len() >= 10000 {
                    return Err(RuntimeError::new(
                        "stack overflow: call depth exceeded 10000",
                    ));
                }
                self.record_call_vm_fast();
                let ctor_frame_idx = self.frames.len();
                let required = frame.base + frame.closure().proto.register_count as usize;
                if self.stack.len() < required {
                    self.stack.resize(required, VmValue::null());
                }
                self.record_frame_push();
                self.frames.push(frame);
                self.pending_constructors
                    .push((ctor_frame_idx, instance_nv));
            }
            PreparedCall::Native(f, args) => {
                self.record_call_native();
                let result =
                    (f)(self as &mut dyn NativeCtx, &args).map_err(|e| RuntimeError::new(e))?;
                self.stack.pop();
                self.push(result);
            }
            PreparedCall::NativeImmediate(f, arg_count) => {
                self.record_call_native();
                let args_start = self.stack.len() - arg_count;

                let result = if arg_count <= 16 {
                    let mut buf = [VmValue::null(); 16];
                    buf[..arg_count]
                        .copy_from_slice(&self.stack[args_start..args_start + arg_count]);
                    self.stack.truncate(args_start - 1);
                    (f)(self as &mut dyn NativeCtx, &buf[..arg_count])
                } else {
                    let vm_args: Vec<VmValue> =
                        self.stack[args_start..args_start + arg_count].to_vec();
                    self.stack.truncate(args_start - 1);
                    (f)(self as &mut dyn NativeCtx, &vm_args)
                }
                .map_err(|e| RuntimeError::new(e))?;

                self.push(result);
            }
            PreparedCall::RawNativeImmediate(f, arg_count) => {
                self.record_call_native();
                let args_start = self.stack.len() - arg_count;

                let result = if arg_count <= 16 {
                    let mut buf = [VmValue::null(); 16];
                    buf[..arg_count]
                        .copy_from_slice(&self.stack[args_start..args_start + arg_count]);
                    self.stack.truncate(args_start - 1);
                    let slice = if arg_count > 0 {
                        &buf[1..arg_count]
                    } else {
                        &buf[..0]
                    };
                    (f)(self as &mut dyn NativeCtx, slice)
                } else {
                    let vm_args: Vec<VmValue> =
                        self.stack[args_start..args_start + arg_count].to_vec();
                    self.stack.truncate(args_start - 1);
                    let slice = if arg_count > 0 {
                        &vm_args[1..]
                    } else {
                        &vm_args[..]
                    };
                    (f)(self as &mut dyn NativeCtx, slice)
                }
                .map_err(|e| RuntimeError::new(e))?;

                self.push(result);
            }
            PreparedCall::NativeConstructor(f, args, instance_nv) => {
                self.record_call_native();
                let result =
                    (f)(self as &mut dyn NativeCtx, &args).map_err(|e| RuntimeError::new(e))?;
                self.stack.pop();
                let nv = if result.is_null() {
                    instance_nv
                } else {
                    result
                };
                self.push(nv);
            }
            PreparedCall::PushValue(nv) => {
                self.stack.pop();
                self.push(nv);
            }
        }
        Ok(())
    }

    pub fn do_return(&mut self, result: VmValue) -> VmResult<VmValue> {
        let returning_frame_idx = self.frames.len().saturating_sub(1);
        let frame = self
            .frames
            .pop()
            .ok_or_else(|| RuntimeError::new("return: no frame"))?;

        while self
            .try_handlers
            .last()
            .map(|h| h.frame_depth > returning_frame_idx)
            .unwrap_or(false)
        {
            self.try_handlers.pop();
        }
        if self.trace {
            varn_utilities::terminal::tagged(
                "vm:return",
                format_args!(
                    "frame={} base={} stack_before={} handlers_before={}",
                    returning_frame_idx,
                    frame.base,
                    self.stack.len(),
                    self.try_handlers.len(),
                ),
            );
        }
        let mut result = result;
        if let Some(exports_nv) = self.module_exports.remove(&returning_frame_idx) {
            result = super::modules::merge_exports(result, exports_nv, &mut self.heap)?;
        }
        self.close_upvalues_above(frame.base);
        if frame.base > 0 {
            self.stack.truncate(frame.base - 1);
        } else {
            self.stack.truncate(0);
        }
        let frame_idx = self.frames.len();
        if let Some(pos) = self
            .pending_setters
            .iter()
            .rposition(|(idx, _)| *idx == frame_idx)
        {
            let (_, assigned) = self.pending_setters.remove(pos);
            self.push(assigned);
            return Ok(assigned);
        }

        let ctor_pos = self
            .pending_constructors
            .iter()
            .rposition(|(idx, _)| *idx == frame_idx);

        if let Some(pos) = ctor_pos {
            let (_, instance_nv) = self.pending_constructors.remove(pos);
            let final_val = if result.is_null() {
                instance_nv
            } else {
                result
            };
            if let Some(reg) = frame.return_reg {
                if let Some(caller_frame) = self.frames.last() {
                    let caller_base = caller_frame.base;
                    self.stack[caller_base + reg as usize] = final_val;
                }
                Ok(final_val)
            } else {
                self.push(final_val);
                Ok(final_val)
            }
        } else if let Some(reg) = frame.return_reg {
            if let Some(caller_frame) = self.frames.last() {
                let caller_base = caller_frame.base;
                self.stack[caller_base + reg as usize] = result;
            }
            Ok(result)
        } else {
            self.push(result);
            if self.trace {
                varn_utilities::terminal::tagged(
                    "vm:return",
                    format_args!(
                        "frame={} result_type={} stack_after={} handlers_after={}",
                        returning_frame_idx,
                        format!("{result:?}"),
                        self.stack.len(),
                        self.try_handlers.len(),
                    ),
                );
            }
            Ok(result)
        }
    }

    pub fn do_return_void(&mut self) -> VmResult<VmValue> {
        self.do_return(VmValue::null())
    }
}

impl NativeCtx for ExecCtx {
    // Native results are runtime-produced values: allocate without interning
    // (`alloc_str` would hash the full contents and retain a reference on the
    // old-gen path — see `alloc_str_dynamic`'s contract).
    fn alloc_str(&mut self, s: &str) -> VmValue {
        self.heap.alloc_str_dynamic(s)
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
        v.is_heap() && matches!(self.heap.get(v.as_heap_idx()), Some(HeapObj::Array(_)))
    }

    fn alloc_array(&mut self, items: Vec<VmValue>) -> VmValue {
        self.heap.alloc_array_vm(items)
    }

    fn array_len(&self, arr: VmValue) -> usize {
        if arr.is_heap() {
            if let Some(HeapObj::Array(a)) = self.heap.get(arr.as_heap_idx()) {
                return a.borrow().len();
            }
        }
        0
    }

    fn array_get(&self, arr: VmValue, idx: usize) -> Option<VmValue> {
        if arr.is_heap() {
            if let Some(HeapObj::Array(a)) = self.heap.get(arr.as_heap_idx()) {
                return a.borrow().get(idx).copied();
            }
        }
        None
    }

    fn array_set(&mut self, arr: VmValue, idx: usize, val: VmValue) {
        if arr.is_heap() {
            let raw_idx = arr.as_heap_idx();
            if let Some(HeapObj::Array(a)) = self.heap.get(raw_idx) {
                let g = a.borrow_mut();
                if idx < g.len() {
                    g[idx] = val;
                    self.heap.write_barrier(raw_idx, val);
                }
            }
        }
    }

    fn array_push(&mut self, arr: VmValue, val: VmValue) {
        if arr.is_heap() {
            let raw_idx = arr.as_heap_idx();
            if let Some(HeapObj::Array(a)) = self.heap.get(raw_idx) {
                a.borrow_mut().push(val);
                self.heap.write_barrier(raw_idx, val);
            }
        }
    }

    fn array_pop(&mut self, arr: VmValue) -> Option<VmValue> {
        if arr.is_heap() {
            if let Some(HeapObj::Array(a)) = self.heap.get(arr.as_heap_idx()) {
                return a.borrow_mut().pop();
            }
        }
        None
    }

    fn array_for_each(&self, arr: VmValue, f: &mut dyn FnMut(VmValue, usize)) {
        if arr.is_heap() {
            if let Some(HeapObj::Array(a)) = self.heap.get(arr.as_heap_idx()) {
                let g = a.borrow();
                for (i, &v) in g.iter().enumerate() {
                    f(v, i);
                }
            }
        }
    }

    fn is_object(&self, v: VmValue) -> bool {
        if v.is_heap() {
            matches!(self.heap.get(v.as_heap_idx()), Some(HeapObj::Object(_)))
        } else {
            false
        }
    }

    fn object_for_each(&self, obj: VmValue, f: &mut dyn FnMut(&str, VmValue)) {
        if obj.is_heap() {
            if let Some(HeapObj::Object(o)) = self.heap.get(obj.as_heap_idx()) {
                let g = o.borrow();
                for (k, v) in g.inner.iter() {
                    f(k.as_ref(), v);
                }
            }
        }
    }

    fn get_object_shape(&self, obj: VmValue) -> Option<std::rc::Rc<varn_types::Shape>> {
        if obj.is_heap() {
            if let Some(HeapObj::Object(o)) = self.heap.get(obj.as_heap_idx()) {
                return Some(o.borrow().inner.shape.clone());
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
            if let Some(HeapObj::Object(o)) = self.heap.get(obj.as_heap_idx()) {
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
                o.borrow_mut().set_field_nv(std::rc::Rc::from(key), val);
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

    fn call_vm(&mut self, callee: VmValue, args: &[VmValue]) -> Result<VmValue, String> {
        self.stack.push(callee);
        self.stack.push(VmValue::null());
        self.stack.extend_from_slice(args);
        let prepared = self
            .prepare_call(callee, args.len() + 1)
            .map_err(|e| e.message)?;
        match prepared {
            PreparedCall::Native(f, args) => {
                let res = (f)(self as &mut dyn NativeCtx, &args);
                self.stack.pop();
                res
            }
            PreparedCall::NativeImmediate(f, arg_count) => {
                let args_start = self.stack.len() - arg_count;
                let vm_args: Vec<VmValue> = self.stack[args_start..args_start + arg_count].to_vec();
                self.stack.drain((args_start - 1)..);
                (f)(self as &mut dyn NativeCtx, &vm_args)
            }
            PreparedCall::RawNativeImmediate(f, arg_count) => {
                let args_start = self.stack.len() - arg_count;
                let vm_args: Vec<VmValue> = self.stack[args_start..args_start + arg_count].to_vec();
                self.stack.drain((args_start - 1)..);
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
                let res = self.run_until(depth).map_err(|e| e.message)?;
                self.stack.pop();
                Ok(res)
            }
            PreparedCall::PushValue(nv) => {
                self.stack.pop();
                Ok(nv)
            }
            PreparedCall::Constructor(frame, instance_nv) => {
                let depth = self.frames.len();
                let required = frame.base + frame.closure().proto.register_count as usize;
                if self.stack.len() < required {
                    self.stack.resize(required, VmValue::null());
                }
                self.frames.push(frame);
                self.pending_constructors.push((depth, instance_nv));
                let _ = self.run_until(depth).map_err(|e| e.message)?;
                self.stack.pop();
                Ok(instance_nv)
            }
            PreparedCall::NativeConstructor(f, args, instance_nv) => {
                let result = (f)(self as &mut dyn NativeCtx, &args).map_err(|e| e)?;
                self.stack.pop();
                let nv = if result.is_null() {
                    instance_nv
                } else {
                    result
                };
                Ok(nv)
            }
        }
    }

    fn spawn_vm(&mut self, callee: VmValue, args: &[VmValue]) -> Result<VmValue, String> {
        let value = self.heap.extract(callee);
        let val_args: Vec<varn_types::Value> = args.iter().map(|&a| self.heap.extract(a)).collect();
        let task = self.spawn_internal(value, &val_args).map_err(|e| e)?;
        Ok(self.heap.intern(task))
    }

    fn set_timer(
        &mut self,
        _ms: u64,
        _repeat: bool,
        _callee: VmValue,
        _args: &[VmValue],
    ) -> Result<usize, String> {
        Err("no timer".into())
    }

    fn clear_timer(&mut self, _id: usize) -> Result<(), String> {
        Ok(())
    }

    fn suspend_timer(&mut self, ms: u64) -> VmValue {
        let output = varn_types::AsyncTask::pending();
        let output_clone = output.clone();

        let spawned = if tokio::runtime::Handle::try_current().is_ok() {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                tokio::task::spawn_local(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                    output_clone.resolve(varn_types::Value::Null);
                });
            }))
            .is_ok()
        } else {
            false
        };

        if !spawned {
            std::thread::sleep(std::time::Duration::from_millis(ms));
            output.resolve(varn_types::Value::Null);
        }
        self.heap.intern(varn_types::Value::TaskHandle(output))
    }

    fn resources(&mut self) -> &mut ResourceStore {
        panic!("ExecCtx::resources() is not implemented in the NaN-boxed VM")
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
        // Heap-independent typed reject payload; the parent's await-resume hook
        // (`host_values::open_rejected`) mints it into a real `Error` on the
        // parent heap so `instanceof Error` works and the message survives (a
        // bare ObjData cannot embed non-SSO strings — see `HostError`).
        fn worker_error(msg: &str) -> varn_types::Value {
            varn_types::value::HostError::to_value("Error", msg)
        }

        let loader = self.loader.clone();

        let module_path_str = module_path.to_string();
        let export_name_str = export_name.to_string();

        // Join task: resolves `Null` when the worker finishes, rejects with a
        // typed error if it threw. Returned to the caller (wrapped in an
        // `IsolateHandle`); no port is injected into the worker.
        let done = varn_types::AsyncTask::pending();
        let done_t = done.clone();

        std::thread::spawn(move || {
            let mut machine = crate::Vm::new(std::rc::Rc::new(rustc_hash::FxHashMap::default()));
            machine
                .ctx
                .globals
                .define("isIsolate", VmValue::from_bool(true));
            if let Some(ld) = loader {
                machine = machine.with_loader(ld);
            }

            if let Err(e) = machine.ctx.load_module("std:task") {
                done_t.reject(worker_error(&format!(
                    "isolate worker: failed to load std:task: {:?}",
                    e
                )));
                return;
            }

            let module_val = match machine.ctx.load_module(&module_path_str) {
                Ok(m) => m,
                Err(e) => {
                    done_t.reject(worker_error(&format!(
                        "isolate worker: failed to load module {}: {:?}",
                        module_path_str, e
                    )));
                    return;
                }
            };

            let func_nv = match machine.ctx.get_field(module_val, &export_name_str) {
                Some(f) => f,
                None => {
                    done_t.reject(worker_error(&format!(
                        "isolate worker: export '{}' not found in module {}",
                        export_name_str, module_path_str
                    )));
                    return;
                }
            };

            // Endpoints arrive as `SendValue::Channel{Sender,Receiver}`;
            // `to_value_ctx` emits `__chanEndpoint` markers, which
            // `host_values::open_resolved` mints into real Sender/Receiver
            // instances (one minting definition, shared with the same-thread
            // await-resume path). std:task is already loaded above, so the
            // endpoint classes exist on this worker's heap.
            let mut vm_args = Vec::new();
            for arg in args {
                let v_nv = arg.to_value_ctx(&mut machine.ctx);
                let val = machine.ctx.heap.extract(v_nv);
                let opened = crate::exec::host_values::open_resolved(&mut machine.ctx, val);
                vm_args.push(machine.ctx.heap.intern(opened));
            }

            match machine.ctx.call_vm(func_nv, &vm_args) {
                Ok(res) => {
                    let val = machine.ctx.heap.extract(res);
                    if let varn_types::Value::Task(lazy) = val {
                        let handle = machine.ctx.run_lazy_task_sync(lazy.as_ref());
                        if let varn_types::task::TaskState::Rejected(e) = handle.peek_state() {
                            done_t.reject(worker_error(&format!("{e}")));
                            return;
                        }
                    }
                    done_t.resolve(varn_types::Value::Null);
                }
                Err(e) => done_t.reject(worker_error(&format!("{e}"))),
            }
        });

        Ok(done)
    }

    fn alloc_instance(&mut self, class_name: &str) -> Option<VmValue> {
        let class_obj = self.get_class(class_name)?;
        let instance_nv = self.heap.alloc_object();
        if let Some(crate::heap::HeapObj::Object(o)) = self.heap.get_mut(instance_nv.as_heap_idx())
        {
            o.borrow_mut().set_class(class_obj);
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
                            .downcast_ref::<crate::frame::VmClosurePayload>()
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
        if val.is_null() {
            return Ok(varn_types::value::SendValue::Null);
        }
        if val.is_bool() {
            return Ok(varn_types::value::SendValue::Bool(val.as_bool()));
        }
        if val.is_int() {
            return Ok(varn_types::value::SendValue::Int(val.as_int()));
        }
        if val.is_f64() {
            return Ok(varn_types::value::SendValue::Float(val.as_f64().to_bits()));
        }
        if val.is_sso() {
            let mut buf = [0u8; 5];
            return Ok(varn_types::value::SendValue::Str(
                val.sso_as_str(&mut buf).to_owned(),
            ));
        }
        if val.is_heap() {
            match self.heap.get_by_idx(val.as_heap_idx()) {
                Some(HeapObj::Str(s)) => Ok(varn_types::value::SendValue::Str(s.to_string())),
                Some(HeapObj::Array(arr)) => {
                    let mut items = Vec::new();
                    for &item in arr.borrow().iter() {
                        items.push(self.to_sendable(item)?);
                    }
                    Ok(varn_types::value::SendValue::Array(items))
                }
                Some(HeapObj::Object(obj)) => {
                    let borrow = obj.borrow();
                    // Channel endpoints (Sender/Receiver instances) transfer by
                    // reference — detected once, in `SendValue::endpoint_for`.
                    if let Some(cls) = borrow.class() {
                        let chan_id = match borrow.get_field("_chan") {
                            Some(varn_types::Value::Int(id)) => Some(id),
                            _ => None,
                        };
                        if let Some(sv) = varn_types::value::SendValue::endpoint_for(
                            cls.name.as_str(),
                            chan_id,
                        )? {
                            return Ok(sv);
                        }
                    }
                    let mut map = std::collections::HashMap::new();
                    for (k, nv) in borrow.inner.iter() {
                        map.insert(k.to_string(), self.to_sendable(nv)?);
                    }
                    Ok(varn_types::value::SendValue::Object(map))
                }
                Some(HeapObj::Map(map_ref)) => {
                    let mut items = Vec::new();
                    for (k, v) in map_ref.read().iter() {
                        items.push((self.value_to_sendable(k)?, self.value_to_sendable(v)?));
                    }
                    Ok(varn_types::value::SendValue::Map(items))
                }
                Some(HeapObj::Set(set_ref)) => {
                    let mut items = Vec::new();
                    for v in set_ref.read().iter() {
                        items.push(self.value_to_sendable(v)?);
                    }
                    Ok(varn_types::value::SendValue::Set(items))
                }
                Some(HeapObj::BigInt(b)) => Ok(varn_types::value::SendValue::BigInt(*b)),
                Some(HeapObj::Decimal(d)) => Ok(varn_types::value::SendValue::Decimal(**d)),
                Some(HeapObj::Char(c)) => Ok(varn_types::value::SendValue::Char(*c)),
                Some(HeapObj::EnumVariant(d)) => {
                    // Payload may hold heap refs — walk it with the
                    // heap-aware converter, not `Value::to_sendable`.
                    let payload = self.value_to_sendable(&d.payload)?;
                    Ok(varn_types::value::SendValue::EnumVariant(Box::new(
                        varn_types::value::SendEnumVariant {
                            enum_name: d.enum_name.to_string(),
                            variant_name: d.variant_name.to_string(),
                            variant_tag: d.variant_tag,
                            fields: d.fields.iter().map(|f| f.to_string()).collect(),
                            payload,
                        },
                    )))
                }
                Some(HeapObj::Range(r)) => {
                    let mut fields = std::collections::HashMap::new();
                    fields.insert(
                        "start".to_string(),
                        varn_types::value::SendValue::Int(r.start),
                    );
                    fields.insert("end".to_string(), varn_types::value::SendValue::Int(r.end));
                    fields.insert(
                        "inclusive".to_string(),
                        varn_types::value::SendValue::Bool(r.inclusive),
                    );
                    fields.insert(
                        "step".to_string(),
                        varn_types::value::SendValue::Int(r.step),
                    );
                    Ok(varn_types::value::SendValue::Object(fields))
                }
                _ => Err("Value cannot be sent to an isolate".to_string()),
            }
        } else {
            Err("Value cannot be sent to an isolate".to_string())
        }
    }
}

impl ExecCtx {
    /// Convert a materialized `Value` (e.g. a Map/Set entry) into a
    /// `SendValue`. Self-contained values route through the single shared
    /// `Value::to_sendable` (which also performs channel-endpoint detection);
    /// objects/arrays/maps/sets and heap refs may hold unresolved `VmValue`
    /// fields, so they walk this heap via [`Self::to_sendable`] — the varn-types
    /// path cannot resolve raw heap indices (`heap.extract` yields a
    /// `Value::Object` whose fields are still `VmValue`s).
    fn value_to_sendable(
        &self,
        val: &varn_types::Value,
    ) -> Result<varn_types::value::SendValue, String> {
        match val {
            varn_types::Value::Object(obj) => {
                let borrow = obj.read();
                if let Some(cls) = borrow.class() {
                    let chan_id = match borrow.get_field("_chan") {
                        Some(varn_types::Value::Int(id)) => Some(id),
                        _ => None,
                    };
                    if let Some(sv) = varn_types::value::SendValue::endpoint_for(
                        cls.name.as_str(),
                        chan_id,
                    )? {
                        return Ok(sv);
                    }
                }
                let mut map = std::collections::HashMap::new();
                for (k, nv) in borrow.inner.iter() {
                    map.insert(k.to_string(), self.to_sendable(nv)?);
                }
                Ok(varn_types::value::SendValue::Object(map))
            }
            varn_types::Value::Array(arr) => {
                let mut items = Vec::new();
                for item in arr.read().iter() {
                    items.push(self.value_to_sendable(item)?);
                }
                Ok(varn_types::value::SendValue::Array(items))
            }
            varn_types::Value::Map(map_ref) => {
                let mut items = Vec::new();
                for (k, v) in map_ref.read().iter() {
                    items.push((self.value_to_sendable(k)?, self.value_to_sendable(v)?));
                }
                Ok(varn_types::value::SendValue::Map(items))
            }
            varn_types::Value::Set(set_ref) => {
                let mut items = Vec::new();
                for v in set_ref.read().iter() {
                    items.push(self.value_to_sendable(v)?);
                }
                Ok(varn_types::value::SendValue::Set(items))
            }
            varn_types::Value::VmValue(payload) => {
                if let Some(vr) = payload.as_any().downcast_ref::<varn_types::VmValueRef>() {
                    self.to_sendable(vr.0)
                } else {
                    Err("Value cannot be sent to an isolate".to_string())
                }
            }
            varn_types::Value::EnumVariant(d) => {
                // Payload may hold heap refs — walk it here instead of
                // delegating to the self-contained `Value::to_sendable`.
                let payload = self.value_to_sendable(&d.payload)?;
                Ok(varn_types::value::SendValue::EnumVariant(Box::new(
                    varn_types::value::SendEnumVariant {
                        enum_name: d.enum_name.to_string(),
                        variant_name: d.variant_name.to_string(),
                        variant_tag: d.variant_tag,
                        fields: d.fields.iter().map(|f| f.to_string()).collect(),
                        payload,
                    },
                )))
            }
            // Scalars and Range are self-contained: one shared conversion.
            _ => val.to_sendable(),
        }
    }

    fn spawn_internal(
        &mut self,
        callee: varn_types::Value,
        args: &[varn_types::Value],
    ) -> Result<varn_types::Value, String> {
        match callee {
            varn_types::Value::Task(t) => {
                let handle = self.run_lazy_task_sync(t.as_ref());
                return Ok(varn_types::Value::TaskHandle(handle));
            }
            varn_types::Value::TaskHandle(f) => {
                return Ok(varn_types::Value::TaskHandle(f));
            }
            _ => {}
        }
        let callee_nv = self.heap.intern(callee);
        let arg_nvs: Vec<_> = args.iter().cloned().map(|a| self.heap.intern(a)).collect();
        let result = self.call_vm(callee_nv, &arg_nvs)?;
        let value = self.heap.extract(result);
        match value {
            varn_types::Value::Task(t) => {
                let handle = self.run_lazy_task_sync(t.as_ref());
                return Ok(varn_types::Value::TaskHandle(handle));
            }
            varn_types::Value::TaskHandle(f) => {
                return Ok(varn_types::Value::TaskHandle(f));
            }
            _ => {}
        }
        let output = varn_types::AsyncTask::pending();
        output.resolve(value);
        Ok(varn_types::Value::TaskHandle(output))
    }
}
