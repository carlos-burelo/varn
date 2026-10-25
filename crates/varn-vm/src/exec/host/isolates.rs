use crate::exec::calls::PreparedCall;
use crate::exec::ctx::ExecCtx;
use crate::heap::HeapObj;
use crate::value::VmValue;
use varn_types::NativeCtx;

pub(crate) enum Invoked {
    Value(VmValue),
    Pushed(usize),
}

impl ExecCtx {
    pub(crate) fn invoke(
        &mut self,
        callee: VmValue,
        window: &[VmValue],
    ) -> crate::error::VmResult<VmValue> {
        if crate::frame::native_stack_low(self.jit_stack_floor) {
            return Err(crate::frame::native_stack_exhausted());
        }
        let was_native = self.heap.cells.suspend_native();
        let result = match self.invoke_pushing(callee, window) {
            Ok(Invoked::Value(v)) => Ok(v),
            Ok(Invoked::Pushed(depth)) => self.run_until(depth),
            Err(e) => Err(e),
        };
        let returned = result
            .as_ref()
            .ok()
            .filter(|v| v.is_heap())
            .map(|v| v.as_heap());
        self.heap.cells.resume(was_native, returned);
        result
    }

    pub(crate) fn invoke_pushing(
        &mut self,
        callee: VmValue,
        window: &[VmValue],
    ) -> crate::error::VmResult<Invoked> {
        self.stage.clear();
        self.stage.extend_from_slice(window);
        let arg_count = window.len();
        let prepared = match self.prepare_call(callee, arg_count) {
            Ok(p) => p,
            Err(e) => {
                self.stage.clear();
                return Err(e);
            }
        };
        self.run_prepared(prepared)
    }

    pub(crate) fn await_call(
        &mut self,
        callee: VmValue,
        window: &[VmValue],
        dest: u16,
    ) -> crate::error::VmResult<crate::exec::ctx_tasks::AwaitStep> {
        self.stage.clear();
        self.stage.extend_from_slice(window);
        let prepared = match self.prepare_call(callee, window.len()) {
            Ok(p) => p,
            Err(e) => {
                self.stage.clear();
                return Err(e);
            }
        };
        let value = match prepared {
            PreparedCall::Async(parts) => {
                self.stage.clear();
                if self.frames.len() >= crate::frame::MAX_CALL_DEPTH {
                    return Err(crate::frame::call_depth_exceeded());
                }
                crate::exec::scheduler::mount_parts(
                    self,
                    &parts.proto,
                    &parts.upvalues,
                    parts.module_base,
                    &parts.args,
                    parts.current_class,
                    dest,
                );
                return Ok(crate::exec::ctx_tasks::AwaitStep::Entered);
            }
            PreparedCall::Frame(_)
            | PreparedCall::Constructor(..)
            | PreparedCall::NativeImmediate(..)
            | PreparedCall::RawNativeImmediate(..)
            | PreparedCall::NativeConstructor(..)
            | PreparedCall::PushValue(_)
            | PreparedCall::Generator { .. } => match self.run_prepared(prepared)? {
                Invoked::Value(v) => v,
                Invoked::Pushed(depth) => self.run_until(depth)?,
            },
        };
        self.begin_await(value, dest)
    }

    fn run_prepared(&mut self, prepared: PreparedCall) -> crate::error::VmResult<Invoked> {
        let res = match prepared {
            PreparedCall::NativeImmediate(f, arg_count) => {
                let take = arg_count.min(self.stage.len());
                let start = self.stage.len() - take;
                let vm_args: Vec<VmValue> = self.stage.drain(start..).collect();
                self.stage.clear();
                self.invoke_native(f, &vm_args)
                    .map_err(crate::error::RuntimeError::from)
            }
            PreparedCall::RawNativeImmediate(f, arg_count) => {
                let take = arg_count.min(self.stage.len());
                let start = self.stage.len() - take;
                let vm_args: Vec<VmValue> = self.stage.drain(start..).collect();
                self.stage.clear();
                let slice = if !vm_args.is_empty() {
                    &vm_args[1..]
                } else {
                    &vm_args[..]
                };
                self.invoke_native(f, slice)
                    .map_err(crate::error::RuntimeError::from)
            }
            PreparedCall::Frame(frame) => {
                let depth = self.frames.len();
                self.frames.push(frame);
                self.stage.clear();
                return Ok(Invoked::Pushed(depth));
            }
            PreparedCall::PushValue(nv) => Ok(nv),
            PreparedCall::Async(parts) => Ok(crate::task::new_lazy(&mut self.heap, parts)),
            PreparedCall::Generator {
                closure,
                args,
                current_class,
            } => Ok(self.build_generator(closure, args, current_class)),
            PreparedCall::Constructor(frame, instance_nv) => {
                let depth = self.frames.len();
                self.frames.push(frame);
                self.pending_constructors.push((depth, instance_nv));
                let _ = self.run_until(depth)?;
                Ok(instance_nv)
            }
            PreparedCall::NativeConstructor(f, args, instance_nv) => {
                let result = self
                    .invoke_native(f, &args)
                    .map_err(crate::error::RuntimeError::from)?;
                let nv = if result.is_null() {
                    instance_nv
                } else {
                    result
                };
                Ok(nv)
            }
        };
        self.stage.clear();
        res.map(Invoked::Value)
    }

    pub(crate) fn task_cell(&self, v: VmValue) -> Option<std::rc::Rc<crate::task::TaskCell>> {
        if !v.is_heap() {
            return None;
        }
        match self.heap.get(v.as_heap()) {
            Some(HeapObj::TaskHandle(cell)) => Some(std::rc::Rc::clone(cell)),
            Some(
                HeapObj::Str(_)
                | HeapObj::Array(_)
                | HeapObj::Tuple(_)
                | HeapObj::Object(_)
                | HeapObj::Record(_)
                | HeapObj::Buffer(_)
                | HeapObj::Module(_)
                | HeapObj::FrozenModule(_)
                | HeapObj::VmClosure(_)
                | HeapObj::Class(_)
                | HeapObj::NativeFn(..)
                | HeapObj::BoundMethod(_)
                | HeapObj::Map(_)
                | HeapObj::Set(_)
                | HeapObj::Task(_)
                | HeapObj::Range(_)
                | HeapObj::EnumVariant(_)
                | HeapObj::BigInt(_)
                | HeapObj::Decimal(_)
                | HeapObj::Char(_)
                | HeapObj::Generator(_)
                | HeapObj::Spread(_),
            )
            | None => None,
        }
    }

    fn spawn_if_task(&mut self, v: VmValue) -> Option<VmValue> {
        if !v.is_heap() {
            return None;
        }
        match self.heap.get(v.as_heap()) {
            Some(HeapObj::Task(lazy)) => {
                let lazy = std::rc::Rc::clone(lazy);
                let output = crate::task::TaskCell::pending();
                crate::exec::scheduler::enqueue_detached(self, lazy, std::rc::Rc::clone(&output));
                Some(crate::task::alloc_handle(&mut self.heap, output))
            }
            Some(HeapObj::TaskHandle(_)) => Some(v),
            Some(
                HeapObj::Str(_)
                | HeapObj::Array(_)
                | HeapObj::Tuple(_)
                | HeapObj::Object(_)
                | HeapObj::Record(_)
                | HeapObj::Buffer(_)
                | HeapObj::Module(_)
                | HeapObj::FrozenModule(_)
                | HeapObj::VmClosure(_)
                | HeapObj::Class(_)
                | HeapObj::NativeFn(..)
                | HeapObj::BoundMethod(_)
                | HeapObj::Map(_)
                | HeapObj::Set(_)
                | HeapObj::Range(_)
                | HeapObj::EnumVariant(_)
                | HeapObj::BigInt(_)
                | HeapObj::Decimal(_)
                | HeapObj::Char(_)
                | HeapObj::Generator(_)
                | HeapObj::Spread(_),
            )
            | None => None,
        }
    }

    pub(super) fn spawn_internal(
        &mut self,
        callee: VmValue,
        args: &[VmValue],
    ) -> Result<VmValue, String> {
        if let Some(handle) = self.spawn_if_task(callee) {
            return Ok(handle);
        }
        let result = self.call_vm(callee, args).map_err(|e| e.message)?;
        if let Some(handle) = self.spawn_if_task(result) {
            return Ok(handle);
        }
        Ok(self.task_resolved(result))
    }
}

pub(super) fn gather_tasks(ctx: &mut ExecCtx, tasks: VmValue) -> Result<VmValue, String> {
    if !ctx.is_array(tasks) {
        return Err("parallel: argument must be an array".to_string());
    }
    let count = ctx.array_len(tasks);
    let results = ctx.heap.alloc_array_vm(vec![VmValue::null(); count]);
    let parent = crate::task::TaskCell::gather(results, count);
    crate::task::track_cell(&mut ctx.heap, &parent);
    let handle = crate::task::alloc_handle(&mut ctx.heap, std::rc::Rc::clone(&parent));
    for index in 0..count {
        let Some(item) = ctx.array_get(tasks, index) else {
            crate::task::gather_one(&mut ctx.heap, &parent, index as u32, Ok(VmValue::null()));
            continue;
        };
        let child = ctx.spawn_internal(item, &[])?;
        match ctx.task_cell(child) {
            Some(cell) => {
                if !cell.gather_into(&parent, index as u32) {
                    let outcome = crate::task::outcome_of_cell(&cell);
                    crate::task::gather_one(&mut ctx.heap, &parent, index as u32, outcome);
                }
            }
            None => {
                crate::task::gather_one(&mut ctx.heap, &parent, index as u32, Ok(child));
            }
        }
    }
    if count == 0 {
        let value = parent.value();
        crate::task::settle(&mut ctx.heap, &parent, Ok(value));
    }
    Ok(handle)
}
