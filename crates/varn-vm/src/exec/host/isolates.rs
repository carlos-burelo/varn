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

    pub(super) fn task_cell(&self, v: VmValue) -> Option<std::rc::Rc<crate::task::TaskCell>> {
        if !v.is_heap() {
            return None;
        }
        match self.heap.get(v.as_heap()) {
            Some(HeapObj::TaskHandle(cell)) => Some(std::rc::Rc::clone(cell)),
            Some(HeapObj::Str(_) | HeapObj::Array(_) | HeapObj::Tuple(_) | HeapObj::Object(_) | HeapObj::Record(_) | HeapObj::Buffer(_) | HeapObj::Module(_) | HeapObj::FrozenModule(_) | HeapObj::VmClosure(_) | HeapObj::Class(_) | HeapObj::NativeFn(..) | HeapObj::BoundMethod(_) | HeapObj::Map(_) | HeapObj::Set(_) | HeapObj::Task(_) | HeapObj::Range(_) | HeapObj::Symbol(_) | HeapObj::EnumVariant(_) | HeapObj::BigInt(_) | HeapObj::Decimal(_) | HeapObj::Char(_) | HeapObj::Generator(_) | HeapObj::Spread(_)) | None => None,
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
            Some(HeapObj::Str(_) | HeapObj::Array(_) | HeapObj::Tuple(_) | HeapObj::Object(_) | HeapObj::Record(_) | HeapObj::Buffer(_) | HeapObj::Module(_) | HeapObj::FrozenModule(_) | HeapObj::VmClosure(_) | HeapObj::Class(_) | HeapObj::NativeFn(..) | HeapObj::BoundMethod(_) | HeapObj::Map(_) | HeapObj::Set(_) | HeapObj::Range(_) | HeapObj::Symbol(_) | HeapObj::EnumVariant(_) | HeapObj::BigInt(_) | HeapObj::Decimal(_) | HeapObj::Char(_) | HeapObj::Generator(_) | HeapObj::Spread(_)) | None => None,
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

pub(super) fn spawn_isolate(
    ctx: &mut ExecCtx,
    module_path: &str,
    export_name: &str,
    args: Vec<varn_types::value::SendValue>,
) -> Result<varn_types::HostPromise, String> {
    fn worker_error(msg: &str) -> varn_types::value::SendValue {
        varn_types::value::SendValue::Error {
            class: "Error".to_string(),
            message: msg.to_string(),
        }
    }

    let loader = ctx.loader.clone();

    let module_path_str = module_path.to_string();
    let export_name_str = export_name.to_string();

    let settings = ctx.settings;

    let done = varn_types::HostPromise::pending();
    let done_t = done.clone();

    std::thread::Builder::new()
        .stack_size(crate::frame::VM_STACK_BYTES)
        .spawn(move || {
            let mut machine =
                crate::Vm::new(std::rc::Rc::new(rustc_hash::FxHashMap::default()), settings);
            machine
                .ctx
                .globals_mut()
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

            let mut vm_args = Vec::new();
            for arg in args {
                let v_nv = arg.to_value_ctx(&mut machine.ctx);
                vm_args.push(varn_builtins::modules::task::mint_endpoint_marker(
                    &mut machine.ctx,
                    v_nv,
                ));
            }

            match machine.ctx.call_vm(func_nv, &vm_args) {
                Ok(res) => {
                    let lazy = if res.is_heap() {
                        match machine.ctx.heap.get(res.as_heap()) {
                            Some(HeapObj::Task(lazy)) => Some(std::rc::Rc::clone(lazy)),
                            Some(HeapObj::Str(_) | HeapObj::Array(_) | HeapObj::Tuple(_) | HeapObj::Object(_) | HeapObj::Record(_) | HeapObj::Buffer(_) | HeapObj::Module(_) | HeapObj::FrozenModule(_) | HeapObj::VmClosure(_) | HeapObj::Class(_) | HeapObj::NativeFn(..) | HeapObj::BoundMethod(_) | HeapObj::Map(_) | HeapObj::Set(_) | HeapObj::TaskHandle(_) | HeapObj::Range(_) | HeapObj::Symbol(_) | HeapObj::EnumVariant(_) | HeapObj::BigInt(_) | HeapObj::Decimal(_) | HeapObj::Char(_) | HeapObj::Generator(_) | HeapObj::Spread(_)) | None => None,
                        }
                    } else {
                        None
                    };
                    if let Some(lazy) = lazy {
                        let cell = machine.ctx.run_lazy_task_sync(lazy);
                        if cell.status() == crate::task::Status::Rejected {
                            let reason = machine.ctx.str_repr(cell.value());
                            done_t.reject(worker_error(&reason));
                            return;
                        }
                    }
                    done_t.resolve(varn_types::value::SendValue::Null);
                }
                Err(e) => done_t.reject(worker_error(&e.to_string())),
            }
        })
        .expect("failed to spawn isolate thread");

    Ok(done)
}
