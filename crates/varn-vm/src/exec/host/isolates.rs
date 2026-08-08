//! Cross-isolate value transfer and the VM-call window.
//!
//! An isolate boundary cannot pass a heap handle: the other side has its own
//! heap and the same index means a different object there. Everything here
//! exists to turn a live `VmValue` into something that survives that crossing
//! (`value_to_sendable`) and to run VM code in a controlled stack window.

use crate::exec::calls::PreparedCall;
use crate::exec::ctx::ExecCtx;
use crate::heap::HeapObj;
use crate::value::VmValue;
use varn_types::NativeCtx;

impl ExecCtx {
    /// Like `call_vm`, but for the JIT call fallback.
    ///
    /// The caller's clif frame flushed its call window to home slots, so the
    /// `arg_count` arguments — including the callee-placeholder null for a
    /// regular call, or the receiver for an extension call — sit at
    /// `stack[src..src + arg_count]`. Copy them to the stack top, where
    /// `prepare_call` expects a callee's frame to begin, and hand `arg_count`
    /// straight over, exactly mirroring the interpreter's `exec_call_reg`
    /// fallback path.
    pub(crate) fn call_vm_window(
        &mut self,
        callee: VmValue,
        src: usize,
        arg_count: usize,
    ) -> Result<VmValue, String> {
        // The window is inside the caller's register file, which is allocated
        // on frame entry — but a proto whose trailing registers were never
        // written can leave the stack short of it.
        if self.stack.len() < src + arg_count {
            self.stack.resize(src + arg_count, VmValue::null());
        }
        let orig_len = self.stack.len();
        self.stack.extend_from_within(src..src + arg_count);
        // `new X()` is the single hottest shape reaching here from clif code.
        // The template JIT's own call helper had this fast path; without it
        // every construction pays prepare_call + a frame push + a nested
        // run_until.
        if callee.is_heap() {
            if let Some(HeapObj::Class(cls)) = self.heap.get(callee.as_heap_idx()) {
                let cls = cls.clone();
                let callee_base = self.stack.len() - arg_count;
                if let Some(v) =
                    crate::exec::jit_helpers::construct_staged_fast(self, &cls, callee_base)
                {
                    self.stack.truncate(orig_len);
                    self.record_call_vm_fast();
                    return Ok(v);
                }
            }
        }
        let prepared = match self.prepare_call(callee, arg_count) {
            Ok(p) => p,
            Err(e) => {
                self.stack.truncate(orig_len);
                return Err(e.message);
            }
        };
        let res = match prepared {
            PreparedCall::NativeImmediate(f, n) => {
                let args_start = self.stack.len() - n;
                let vm_args: Vec<VmValue> = self.stack[args_start..args_start + n].to_vec();
                (f)(self as &mut dyn NativeCtx, &vm_args)
            }
            PreparedCall::RawNativeImmediate(f, n) => {
                let args_start = self.stack.len() - n;
                let vm_args: Vec<VmValue> = self.stack[args_start..args_start + n].to_vec();
                let slice = if n > 0 { &vm_args[1..] } else { &vm_args[..] };
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

    /// Convert a materialized `Value` (e.g. a Map/Set entry) into a
    /// `SendValue`. Self-contained values route through the single shared
    /// `Value::to_sendable` (which also performs channel-endpoint detection);
    /// objects/arrays/maps/sets and heap refs may hold unresolved `VmValue`
    /// fields, so they walk this heap via [`Self::to_sendable`] — the varn-types
    /// path cannot resolve raw heap indices (`heap.extract` yields a
    /// `Value::Object` whose fields are still `VmValue`s).
    pub(super) fn value_to_sendable(
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
                    if let Some(sv) =
                        varn_types::value::SendValue::endpoint_for(cls.name.as_str(), chan_id)?
                    {
                        return Ok(sv);
                    }
                }
                let mut map = std::collections::HashMap::new();
                for (k, nv) in borrow.iter() {
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
                    items.push((self.to_sendable(k.0)?, self.to_sendable(*v)?));
                }
                Ok(varn_types::value::SendValue::Map(items))
            }
            varn_types::Value::Set(set_ref) => {
                let mut items = Vec::new();
                for k in set_ref.read().iter() {
                    items.push(self.to_sendable(k.0)?);
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

    pub(super) fn spawn_internal(
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

/// Body of [`NativeCtx::spawn_isolate`]. Lives here rather than in the trait
/// impl because starting an isolate is the isolate domain, not the shape of
/// the host boundary.
pub(super) fn spawn_isolate(
    ctx: &mut ExecCtx,
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
    let done = varn_types::AsyncTask::pending();
    let done_t = done.clone();

    std::thread::spawn(move || {
        let mut machine =
            crate::Vm::new(std::rc::Rc::new(rustc_hash::FxHashMap::default()), settings);
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
                    let chan_id = match borrow.get_field("_chan") {
                        Some(varn_types::Value::Int(id)) => Some(id),
                        _ => None,
                    };
                    if let Some(sv) =
                        varn_types::value::SendValue::endpoint_for(cls.name.as_str(), chan_id)?
                    {
                        return Ok(sv);
                    }
                }
                let mut map = std::collections::HashMap::new();
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
            Some(HeapObj::BigInt(b)) => Ok(varn_types::value::SendValue::BigInt(*b)),
            Some(HeapObj::Decimal(d)) => Ok(varn_types::value::SendValue::Decimal(**d)),
            Some(HeapObj::Char(c)) => Ok(varn_types::value::SendValue::Char(*c)),
            Some(HeapObj::EnumVariant(d)) => {
                // Payload may hold heap refs — walk it with the
                // heap-aware converter, not `Value::to_sendable`.
                let payload = ctx.value_to_sendable(&d.payload)?;
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
