//! Cross-isolate value transfer and the VM-call window.
//!
//! An isolate boundary cannot pass a heap handle: the other side has its own
//! heap and the same index means a different object there. Everything here
//! exists to turn a live `VmValue` into something that survives that crossing
//! to run VM code in a controlled stack window.

use crate::exec::calls::PreparedCall;
use crate::exec::ctx::ExecCtx;
use crate::heap::HeapObj;
use crate::value::VmValue;
use varn_types::NativeCtx;

impl ExecCtx {
    /// THE canonical VM invocation: run `callee` to completion with a boxed
    /// `window` whose first slot is the callee's placeholder (the register the
    /// interpreter's callee slot / the compiled caller's staging register
    /// occupies), followed by the arguments.
    ///
    /// Every run-to-completion entry into VM code goes through here:
    /// `jit_invoke_dynamic` (compiled caller), `NativeCtx::call_vm` (host →
    /// VM), `spawn_internal`, and isolates. The window is adopted as staging,
    /// so it materialises through the SAME `prepare_call`/`materialize_frame`
    /// as the interpreter's slow path — one argument convention, not two.
    ///
    /// Fase A del frame por clases: el llamador compilado ya no puede exponer
    /// su ventana de argumentos como tramo contiguo del almacén — los home
    /// slots viven en los vectores por clase (`FrameStore`) y solo el valor
    /// completo tiene sentido fuera del frame. La ventana llega entonces
    /// boxeada en `window`, con el callee/placeholder en el primer slot, y se
    /// trata como staging.
    pub(crate) fn invoke(
        &mut self,
        callee: VmValue,
        window: &[VmValue],
    ) -> crate::error::VmResult<VmValue> {
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
                // La ventana vive al FINAL de staging (ver dispatch_prepared_call).
                let take = arg_count.min(self.stage.len());
                let start = self.stage.len() - take;
                let vm_args: Vec<VmValue> = self.stage.drain(start..).collect();
                self.stage.clear();
                (f)(self as &mut dyn NativeCtx, &vm_args).map_err(crate::error::RuntimeError::from)
            }
            PreparedCall::RawNativeImmediate(f, arg_count) => {
                let take = arg_count.min(self.stage.len());
                let start = self.stage.len() - take;
                let vm_args: Vec<VmValue> = self.stage.drain(start..).collect();
                self.stage.clear();
                let slice = if vm_args.len() > 0 {
                    &vm_args[1..]
                } else {
                    &vm_args[..]
                };
                (f)(self as &mut dyn NativeCtx, slice).map_err(crate::error::RuntimeError::from)
            }
            PreparedCall::Frame(frame) => {
                // El frame ya trae su región tipada (`materialize_frame`):
                // solo entra en la lista.
                let depth = self.frames.len();
                self.frames.push(frame);
                self.run_until(depth)
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
                let result = (f)(self as &mut dyn NativeCtx, &args)
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
        res
    }

    pub(super) fn task_cell(&self, v: VmValue) -> Option<std::rc::Rc<crate::task::TaskCell>> {
        if !v.is_heap() {
            return None;
        }
        match self.heap.get_by_idx(v.as_heap_idx()) {
            Some(HeapObj::TaskHandle(cell)) => Some(std::rc::Rc::clone(cell)),
            _ => None,
        }
    }

    fn spawn_if_task(&mut self, v: VmValue) -> Option<VmValue> {
        if !v.is_heap() {
            return None;
        }
        match self.heap.get_by_idx(v.as_heap_idx()) {
            Some(HeapObj::Task(lazy)) => {
                let lazy = std::rc::Rc::clone(lazy);
                let output = crate::task::TaskCell::pending();
                crate::exec::scheduler::enqueue_detached(self, lazy, std::rc::Rc::clone(&output));
                Some(crate::task::alloc_handle(&mut self.heap, output))
            }
            Some(HeapObj::TaskHandle(_)) => Some(v),
            _ => None,
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

/// Body of [`NativeCtx::spawn_isolate`]. Lives here rather than in the trait
/// impl because starting an isolate is the isolate domain, not the shape of
/// the host boundary.
pub(super) fn spawn_isolate(
    ctx: &mut ExecCtx,
    module_path: &str,
    export_name: &str,
    args: Vec<varn_types::value::SendValue>,
) -> Result<varn_types::HostPromise, String> {
    // Heap-independent typed reject payload; the parent's await-resume hook
    // (`host_values::open_rejected`) mints it into a real `Error` on the
    // parent heap so `instanceof Error` works and the message survives (a
    // bare ObjData cannot embed non-SSO strings — see `HostError`).
    fn worker_error(msg: &str) -> varn_types::value::SendValue {
        varn_types::value::SendValue::Error {
            class: "Error".to_string(),
            message: msg.to_string(),
        }
    }

    let loader = ctx.loader.clone();

    let module_path_str = module_path.to_string();
    let export_name_str = export_name.to_string();
    // The worker gets a fresh VM, so it must be handed this VM's settings;
    // otherwise an interpreter-only run is not actually interpreter-only
    // inside isolates.
    let settings = ctx.settings;

    // Join task: resolves `Null` when the worker finishes, rejects with a
    // typed error if it threw. Returned to the caller (wrapped in an
    // `IsolateHandle`); no port is injected into the worker.
    let done = varn_types::HostPromise::pending();
    let done_t = done.clone();

    std::thread::spawn(move || {
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

        // Endpoints arrive as `SendValue::Channel{Sender,Receiver}`;
        // `to_value_ctx` emits `__chanEndpoint` markers, which
        // `host_values::open_resolved` mints into real Sender/Receiver
        // instances (one minting definition, shared with the same-thread
        // await-resume path). std:task is already loaded above, so the
        // endpoint classes exist on this worker's heap.
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
                    match machine.ctx.heap.get_by_idx(res.as_heap_idx()) {
                        Some(HeapObj::Task(lazy)) => Some(std::rc::Rc::clone(lazy)),
                        _ => None,
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
    });

    Ok(done)
}

/// Body of [`NativeCtx::to_sendable`]: a heap handle means nothing on the
/// other side of an isolate boundary, so every value has to be copied out
/// into a heap-independent `SendValue` before it can cross.
pub(super) fn to_sendable(
    ctx: &ExecCtx,
    val: VmValue,
) -> Result<varn_types::value::SendValue, String> {
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
        match ctx.heap.get_by_idx(val.as_heap_idx()) {
            Some(HeapObj::Str(s)) => Ok(varn_types::value::SendValue::Str(s.to_string())),
            Some(HeapObj::Array(arr)) => {
                let mut items = Vec::with_capacity(arr.len());
                for i in 0..arr.len() {
                    // `get_vm` boxes on read for typed reprs — a typed
                    // array crossing an isolate boundary serializes the
                    // same as a Boxed one; no migration, read-only.
                    items.push(ctx.to_sendable(arr.get_vm(i).unwrap())?);
                }
                Ok(varn_types::value::SendValue::Array(items))
            }
            Some(HeapObj::Object(obj)) => {
                let borrow = obj.borrow();
                // Channel endpoints (Sender/Receiver instances) transfer by
                // reference — detected once, in `SendValue::endpoint_for`.
                if let Some(cls) = borrow.class() {
                    let chan_id = borrow
                        .get("_chan")
                        .filter(|v| v.is_int())
                        .map(|v| v.as_int());
                    if let Some(sv) =
                        varn_types::value::SendValue::endpoint_for(cls.name.as_str(), chan_id)?
                    {
                        return Ok(sv);
                    }
                }
                let mut map = rustc_hash::FxHashMap::default();
                for (k, nv) in borrow.iter() {
                    map.insert(k.to_string(), ctx.to_sendable(nv)?);
                }
                Ok(varn_types::value::SendValue::Object(map))
            }
            Some(HeapObj::Map(map_ref)) => {
                let map_ref = map_ref.clone();
                let mut items = Vec::new();
                for (k, v) in map_ref.read().iter() {
                    items.push((ctx.to_sendable(k.0)?, ctx.to_sendable(*v)?));
                }
                Ok(varn_types::value::SendValue::Map(items))
            }
            Some(HeapObj::Set(set_ref)) => {
                let set_ref = set_ref.clone();
                let mut items = Vec::new();
                for v in set_ref.read().iter() {
                    items.push(ctx.to_sendable(v.0)?);
                }
                Ok(varn_types::value::SendValue::Set(items))
            }
            Some(HeapObj::BigInt(b)) => Ok(varn_types::value::SendValue::BigInt((**b).clone())),
            Some(HeapObj::Decimal(d)) => Ok(varn_types::value::SendValue::Decimal((**d).clone())),
            Some(HeapObj::Char(c)) => Ok(varn_types::value::SendValue::Char(*c)),
            Some(HeapObj::EnumVariant(d)) => {
                let payload = ctx.to_sendable(d.payload)?;
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
                let mut fields = rustc_hash::FxHashMap::default();
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
