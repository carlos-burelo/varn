use crate::error::{RuntimeError, VmResult};
use crate::frame::CallFrame;
use crate::heap::HeapObj;
use crate::value::VmValue;
use varn_types::value::{ObjData, ObjRef};
use varn_types::{ClassObj, NativeCtx, NativeFn, ResourceStore, VmArray};

use super::calls::PreparedCall;
use super::ctx::ExecCtx;

impl ExecCtx {
    pub(crate) fn dispatch_prepared_call(&mut self, call: PreparedCall) -> VmResult<()> {
        match call {
            PreparedCall::Frame(frame) => {
                self.record_call_vm_fast();
                let required = frame.base + frame.closure.proto.register_count as usize;
                if self.stack.len() < required {
                    self.stack.resize(required, VmValue::null());
                }
                self.record_frame_push();
                self.frames.push(frame);

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
                        for c in frame.closure.constants.iter() {
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
                self.record_call_vm_fast();
                let ctor_frame_idx = self.frames.len();
                let required = frame.base + frame.closure.proto.register_count as usize;
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
                    self.stack.drain((args_start - 1)..);
                    (f)(self as &mut dyn NativeCtx, &buf[..arg_count])
                } else {
                    let vm_args: Vec<VmValue> =
                        self.stack[args_start..args_start + arg_count].to_vec();
                    self.stack.drain((args_start - 1)..);
                    (f)(self as &mut dyn NativeCtx, &vm_args)
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

                let final_nv = if nv.is_heap() {
                    if let Some(crate::heap::HeapObj::Task(lazy)) = self.heap.get(nv.as_heap_idx())
                    {
                        let lazy = lazy.clone();
                        let handle = self.run_lazy_task_sync(lazy.as_ref());
                        let resolved = match handle.peek_state() {
                            varn_types::task::TaskState::Resolved(v) => v,
                            _ => varn_types::Value::Null,
                        };
                        self.heap.intern(resolved)
                    } else {
                        nv
                    }
                } else {
                    nv
                };
                self.push(final_nv);
            }
        }
        Ok(())
    }

    pub(crate) fn dispatch_prepared_setter_call(
        &mut self,
        call: PreparedCall,
        assigned_value: VmValue,
    ) -> VmResult<()> {
        match call {
            PreparedCall::Frame(frame) => {
                self.record_call_vm_fast();
                let setter_frame_idx = self.frames.len();
                let required = frame.base + frame.closure.proto.register_count as usize;
                if self.stack.len() < required {
                    self.stack.resize(required, VmValue::null());
                }
                self.frames.push(frame);
                self.pending_setters
                    .push((setter_frame_idx, assigned_value));
            }
            PreparedCall::Native(f, args) => {
                self.record_call_native();
                let _ = (f)(self as &mut dyn NativeCtx, &args).map_err(RuntimeError::new)?;
                self.stack.pop();
                self.push(assigned_value);
            }
            other => {
                self.dispatch_prepared_call(other)?;
                let _ = self.pop()?;
                self.push(assigned_value);
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
            eprintln!(
                "[vm:return] frame={} base={} stack_before={} handlers_before={}",
                returning_frame_idx,
                frame.base,
                self.stack.len(),
                self.try_handlers.len(),
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
                eprintln!(
                    "[vm:return] frame={} result_type={} stack_after={} handlers_after={}",
                    returning_frame_idx,
                    format!("{result:?}"),
                    self.stack.len(),
                    self.try_handlers.len(),
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
    fn alloc_str(&mut self, s: &str) -> VmValue {
        self.heap.alloc_str(s)
    }

    fn alloc_str_owned(&mut self, s: String) -> VmValue {
        self.heap.alloc_str(&s)
    }

    fn str_repr(&self, v: VmValue) -> String {
        self.heap.str_repr(v)
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
        let va = varn_types::VmArray::new(items);
        VmValue::from_heap_idx(self.heap.alloc(HeapObj::Array(va)))
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
            if let Some(HeapObj::Array(a)) = self.heap.get(arr.as_heap_idx()) {
                let mut g = a.borrow_mut();
                if idx < g.len() {
                    g[idx] = val;
                }
            }
        }
    }

    fn array_push(&mut self, arr: VmValue, val: VmValue) {
        if arr.is_heap() {
            if let Some(HeapObj::Array(a)) = self.heap.get(arr.as_heap_idx()) {
                a.borrow_mut().push(val);
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

    fn alloc_object(&mut self) -> VmValue {
        self.heap.alloc_object()
    }

    fn get_field(&self, obj: VmValue, key: &str) -> Option<VmValue> {
        if obj.is_heap() {
            if let Some(HeapObj::Object(o)) = self.heap.get(obj.as_heap_idx()) {
                return o.borrow().get_field_nv(key);
            }

            if let Some(HeapObj::NativeModule(map)) = self.heap.get(obj.as_heap_idx()) {
                return map.get(key).copied();
            }
        }
        None
    }

    fn set_field(&mut self, obj: VmValue, key: &str, val: VmValue) {
        if obj.is_heap() {
            if let Some(HeapObj::Object(o)) = self.heap.get(obj.as_heap_idx()) {
                o.borrow_mut().set_field_nv(std::rc::Rc::from(key), val);
            } else if let Some(HeapObj::NativeModule(map_rc)) = self.heap.get_mut(obj.as_heap_idx())
            {
                std::rc::Rc::make_mut(map_rc).insert(std::rc::Rc::from(key), val);
            } else {
                eprintln!(
                    "[set_field] MISS key={key} obj_heap_idx={} is_heap={}",
                    obj.as_heap_idx(),
                    obj.is_heap()
                );
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
        self.stack.extend_from_slice(args);
        let prepared = self
            .prepare_call(callee, args.len())
            .map_err(|e| e.message)?;
        match prepared {
            PreparedCall::Native(f, args) => (f)(self as &mut dyn NativeCtx, &args),
            PreparedCall::NativeImmediate(f, arg_count) => {
                let args_start = self.stack.len() - arg_count;
                let vm_args: Vec<VmValue> = self.stack[args_start..args_start + arg_count].to_vec();
                self.stack.drain((args_start - 1)..);
                (f)(self as &mut dyn NativeCtx, &vm_args)
            }
            PreparedCall::Frame(frame) => {
                let depth = self.frames.len();
                let required = frame.base + frame.closure.proto.register_count as usize;
                if self.stack.len() < required {
                    self.stack.resize(required, VmValue::null());
                }
                self.frames.push(frame);
                let res = self.run_until(depth).map_err(|e| e.message)?;
                Ok(res)
            }
            PreparedCall::PushValue(nv) => {
                self.stack.pop();
                Ok(nv)
            }
            _ => Err("unsupported call from NativeCtx".into()),
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
        std::thread::sleep(std::time::Duration::from_millis(ms));
        output.resolve(varn_types::Value::Null);
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
}

impl ExecCtx {
    fn spawn_internal(
        &mut self,
        callee: varn_types::Value,
        args: &[varn_types::Value],
    ) -> Result<varn_types::Value, String> {
        if args.is_empty() {
            return self.start_task_internal(callee);
        }
        let callee_nv = self.heap.intern(callee);
        let arg_nvs: Vec<_> = args.iter().cloned().map(|a| self.heap.intern(a)).collect();
        self.stack.extend(arg_nvs);
        let prepared = self
            .prepare_call(callee_nv, args.len())
            .map_err(|e| e.message)?;
        self.dispatch_prepared_call(prepared)
            .map_err(|e| e.message)?;
        let result = self.stack.pop().unwrap_or(VmValue::null());
        let value = self.heap.extract(result);
        let output = varn_types::AsyncTask::pending();
        output.resolve(value);
        Ok(varn_types::Value::TaskHandle(output))
    }

    fn start_task_internal(
        &mut self,
        task: varn_types::Value,
    ) -> Result<varn_types::Value, String> {
        match task {
            varn_types::Value::Task(t) => {
                let handle = self.run_lazy_task_sync(t.as_ref());
                Ok(varn_types::Value::TaskHandle(handle))
            }
            varn_types::Value::TaskHandle(f) => Ok(varn_types::Value::TaskHandle(f)),
            other => Err(format!("expected Task, got {}", other.type_name())),
        }
    }
}
