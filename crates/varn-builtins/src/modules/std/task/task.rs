use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, Value, VmValue};

pub struct TaskRuntime;

pub struct IsolatePortImpl;

varn_contract! {
    module: "runtime:task",
    contract: "src/modules/std/task/runtime/task_runtime.vn",
    impl TaskRuntime {
        fn taskSpawn(ctx: &mut dyn NativeCtx, target: VmValue, args: &[VmValue]) -> Result<VmValue, String> {
            ctx.spawn_vm(target, args)
        }

        fn taskSleep(ctx: &mut dyn NativeCtx, ms: i64) -> Result<VmValue, String> {
            Ok(ctx.suspend_timer(ms.max(0) as u64))
        }

        fn taskParallel(ctx: &mut dyn NativeCtx, tasks: VmValue) -> Result<VmValue, String> {
            if !ctx.is_array(tasks) {
                return Err("parallel: argument must be an array".to_string());
            }
            let len = ctx.array_len(tasks);
            let mut handles = Vec::with_capacity(len);
            for i in 0..len {
                if let Some(item_nv) = ctx.array_get(tasks, i) {
                    handles.push(ctx.spawn_vm(item_nv, &[])?);
                }
            }

            use std::sync::{Arc, Mutex};
            let output = varn_types::AsyncTask::pending();
            let pending_count = Arc::new(Mutex::new(handles.len()));
            let results = Arc::new(Mutex::new(vec![Value::Null; handles.len()]));

            for (idx, handle_nv) in handles.iter().copied().enumerate() {
                let handle_val = ctx.extract(handle_nv);
                if let Value::TaskHandle(handle) = handle_val {
                    let pending_count = Arc::clone(&pending_count);
                    let results = Arc::clone(&results);
                    let output_clone = output.clone();
                    handle.on_settle(move |res| {
                        let val = match res {
                            Ok(v) => v,
                            Err(e) => {
                                output_clone.reject_msg(format!("{e}"));
                                return;
                            }
                        };
                        let mut results_guard = results.lock().unwrap();
                        results_guard[idx] = val;
                        let mut count_guard = pending_count.lock().unwrap();
                        *count_guard -= 1;
                        if *count_guard == 0 {
                            let arr = varn_types::value::ArrayRef::new(results_guard.clone());
                            output_clone.resolve(Value::Array(arr));
                        }
                    });
                } else {
                    let mut count_guard = pending_count.lock().unwrap();
                    *count_guard -= 1;
                    if *count_guard == 0 {
                        let results_guard = results.lock().unwrap();
                        let arr = varn_types::value::ArrayRef::new(results_guard.clone());
                        output.resolve(Value::Array(arr));
                    }
                }
            }

            if handles.is_empty() {
                let arr = varn_types::value::ArrayRef::new(Vec::new());
                output.resolve(Value::Array(arr));
            }
            Ok(ctx.intern(Value::TaskHandle(output)))
        }

        fn spawnIsolate(ctx: &mut dyn NativeCtx, func: VmValue, args: VmValue) -> Result<VmValue, String> {
            let (resolved_path, export_name) = ctx
                .get_function_location(func)
                .ok_or_else(|| "spawnIsolate: first argument must be a function reference".to_string())?;

            let module_val = match ctx.load_module(&resolved_path) {
                Ok(m) => m,
                Err(e) => {
                    let async_task = varn_types::AsyncTask::pending();
                    let mut obj = varn_types::value::ObjData::new();
                    let err_msg = format!("spawnIsolate: failed to load module '{resolved_path}': {e}");
                    let err_msg_nv = ctx.alloc_str(&err_msg);
                    let err_msg_val = Value::VmValue(Box::new(varn_types::VmValueRef(err_msg_nv)));
                    obj.set_field(std::rc::Rc::from("message"), err_msg_val);
                    async_task.reject(varn_types::value::new_object(obj));
                    return Ok(ctx.intern(Value::TaskHandle(async_task)));
                }
            };

            let exported_fn = ctx.get_field(module_val, &export_name);
            if exported_fn != Some(func) {
                let async_task = varn_types::AsyncTask::pending();
                let mut obj = varn_types::value::ObjData::new();
                let err_msg = if export_name.starts_with('<') {
                    "spawnIsolate: first argument must be a function reference, not an anonymous closure".to_string()
                } else {
                    format!("spawnIsolate: function '{export_name}' is not a top-level exported function of module '{resolved_path}'")
                };
                let err_msg_nv = ctx.alloc_str(&err_msg);
                let err_msg_val = Value::VmValue(Box::new(varn_types::VmValueRef(err_msg_nv)));
                obj.set_field(std::rc::Rc::from("message"), err_msg_val);
                async_task.reject(varn_types::value::new_object(obj));
                return Ok(ctx.intern(Value::TaskHandle(async_task)));
            }

            if !ctx.is_array(args) {
                return Err("spawnIsolate: arguments must be an array".to_string());
            }
            let mut worker_args = Vec::new();
            let len = ctx.array_len(args);
            for i in 0..len {
                if let Some(item_nv) = ctx.array_get(args, i) {
                    worker_args.push(ctx.to_sendable(item_nv)?);
                }
            }

            let (port_parent, port_child) = varn_runtime::isolate::IsolatePort::new();
            ctx.spawn_isolate(&resolved_path, &export_name, worker_args, Box::new(port_child))?;

            let instance_nv = ctx
                .alloc_instance("IsolatePort")
                .ok_or("Failed to instantiate IsolatePort")?;
            let port_nv = ctx.intern(Value::VmValue(Box::new(port_parent)));
            ctx.set_field(instance_nv, "_port", port_nv);
            Ok(instance_nv)
        }

        fn channel(ctx: &mut dyn NativeCtx, capacity: i64) -> Result<VmValue, String> {
            if capacity < 1 {
                return Err("channel: capacity must be >= 1".to_string());
            }
            let id = varn_runtime::channel::create(capacity as usize);
            let ch_nv = ctx
                .alloc_instance("Channel")
                .ok_or("channel: Channel class not registered")?;
            let tx_nv = alloc_endpoint(ctx, "Sender", id)?;
            let rx_nv = alloc_endpoint(ctx, "Receiver", id)?;
            ctx.set_field(ch_nv, "tx", tx_nv);
            ctx.set_field(ch_nv, "rx", rx_nv);
            Ok(ch_nv)
        }
    }
}

varn_contract! {
    module: "runtime:task",
    class: "IsolatePort",
    contract: "src/modules/std/task/runtime/task_runtime.vn",
    impl IsolatePortImpl {
        fn send(ctx: &mut dyn NativeCtx, this: VmValue, msg: VmValue) {
            let Ok(send_val) = ctx.to_sendable(msg) else { return };
            let Some(port_nv) = ctx.get_field(this, "_port") else { return };
            if let Value::VmValue(payload) = ctx.extract(port_nv) {
                if let Some(port) =
                    payload.as_any().downcast_ref::<varn_runtime::isolate::IsolatePort>()
                {
                    let _ = port.send(send_val);
                }
            }
        }

        fn receive(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            let Some(port_nv) = ctx.get_field(this, "_port") else {
                return VmValue::null();
            };
            if let Value::VmValue(payload) = ctx.extract(port_nv) {
                if let Some(port) =
                    payload.as_any().downcast_ref::<varn_runtime::isolate::IsolatePort>()
                {
                    let rx_guard = port.rx.lock().unwrap();
                    let async_task = varn_types::AsyncTask::pending();
                    match rx_guard.try_recv() {
                        Ok(send_val) => {
                            let val_nv = send_val.to_value_ctx(ctx);
                            let v = ctx.extract(val_nv);
                            async_task.resolve(v);
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => {
                            *port.waker.lock().unwrap() = Some(async_task.clone());
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            async_task.resolve(Value::Null);
                        }
                    }
                    return ctx.intern(Value::TaskHandle(async_task));
                }
            }
            VmValue::null()
        }
    }
}

// ---------------------------------------------------------------------------
// Typed channels: Sender / Receiver / Channel / ChannelClosed (runtime:task).
// ---------------------------------------------------------------------------

pub struct SenderImpl;
pub struct ReceiverImpl;
pub struct ChannelImpl;
pub struct ChannelClosedImpl;

/// Allocate a `Sender`/`Receiver` instance holding only the channel `_chan` id.
/// A `Receiver` also gets a self-returning `Symbol.asyncIterator` so `for await`
/// drives it directly through its own `next()`. Shared with the VM's
/// `host_values::mint_endpoint` (cross-isolate materialization) so both minting
/// paths produce identical instances.
pub fn alloc_endpoint(
    ctx: &mut dyn NativeCtx,
    class_name: &str,
    id: u64,
) -> Result<VmValue, String> {
    let nv = ctx
        .alloc_instance(class_name)
        .ok_or_else(|| format!("channel: {class_name} class not registered"))?;
    ctx.set_field(nv, "_chan", VmValue::from_int(id as i64));
    if class_name == "Receiver" {
        // for-await: Symbol.asyncIterator returns the receiver itself
        // (self-iterator), whose `next()` yields `{value, done}`.
        let self_val = ctx.extract(nv);
        let iter_nv = ctx.intern(Value::native_bound(
            self_val,
            receiver_self_iterator,
            "[Symbol.asyncIterator]",
        ));
        ctx.set_field(nv, "Symbol.asyncIterator", iter_nv);
    }
    Ok(nv)
}

fn receiver_self_iterator(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
    args.first()
        .copied()
        .ok_or_else(|| "receiver iterator: missing self".to_string())
}

fn chan_id(ctx: &mut dyn NativeCtx, this: VmValue) -> Option<u64> {
    let nv = ctx.get_field(this, "_chan")?;
    match ctx.extract(nv) {
        Value::Int(i) => Some(i as u64),
        _ => None,
    }
}

/// Heap-independent typed error payload; the VM's `host_values::open_rejected`
/// mints it as a real `ChannelClosed` instance on the consumer's heap (works
/// same-thread and cross-thread). A marker *object* would lose its strings:
/// bare `ObjData::set_field` cannot embed non-SSO strings — see
/// `varn_types::value::HostError`.
fn closed_error_obj(msg: &str) -> Value {
    varn_types::value::HostError::to_value("ChannelClosed", msg)
}

/// Build a `{value, done}` result object on the consumer's heap. Unlike
/// `channel::next_obj`, this routes through `ctx` so composite values embed
/// correctly (a bare `ObjData::set_field` only accepts scalars).
fn next_result(ctx: &mut dyn NativeCtx, value_nv: VmValue, done: bool) -> VmValue {
    let obj = ctx.alloc_object();
    ctx.set_field(obj, "value", value_nv);
    let done_nv = ctx.bool_val(done);
    ctx.set_field(obj, "done", done_nv);
    obj
}

varn_contract! {
    module: "runtime:task",
    class: "Sender",
    contract: "src/modules/std/task/runtime/task_runtime.vn",
    impl SenderImpl {
        fn send(ctx: &mut dyn NativeCtx, this: VmValue, msg: VmValue) -> VmValue {
            let out = varn_types::AsyncTask::pending();
            let Some(id) = chan_id(ctx, this) else {
                out.reject(closed_error_obj("channel closed"));
                return ctx.intern(Value::TaskHandle(out));
            };
            let send_val = match ctx.to_sendable(msg) {
                Ok(v) => v,
                Err(e) => {
                    out.reject_msg(format!("send: {e}"));
                    return ctx.intern(Value::TaskHandle(out));
                }
            };
            match varn_runtime::channel::send(id, send_val) {
                varn_runtime::channel::SendOutcome::Sent => out.resolve(Value::Null),
                varn_runtime::channel::SendOutcome::Closed => {
                    out.reject(closed_error_obj("channel closed"));
                }
                varn_runtime::channel::SendOutcome::Parked(task) => {
                    let out2 = out.clone();
                    task.on_settle(move |res| match res {
                        Ok(Value::Bool(true)) => out2.resolve(Value::Null),
                        _ => out2.reject(closed_error_obj("channel closed")),
                    });
                }
            }
            ctx.intern(Value::TaskHandle(out))
        }

        fn close(ctx: &mut dyn NativeCtx, this: VmValue) {
            if let Some(id) = chan_id(ctx, this) {
                varn_runtime::channel::close(id);
            }
        }

        fn dispose(ctx: &mut dyn NativeCtx, this: VmValue) {
            if let Some(id) = chan_id(ctx, this) {
                varn_runtime::channel::close(id);
            }
        }
    }
}

varn_contract! {
    module: "runtime:task",
    class: "Receiver",
    contract: "src/modules/std/task/runtime/task_runtime.vn",
    impl ReceiverImpl {
        fn next(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            let out = varn_types::AsyncTask::pending();
            let Some(id) = chan_id(ctx, this) else {
                out.resolve(varn_runtime::channel::next_obj(Value::Null, true));
                return ctx.intern(Value::TaskHandle(out));
            };
            match varn_runtime::channel::try_receive(id) {
                varn_runtime::channel::RecvOutcome::Item(v) => {
                    let val_nv = v.to_value_ctx(ctx);
                    let res_nv = next_result(ctx, val_nv, false);
                    let res = ctx.extract(res_nv);
                    out.resolve(res);
                }
                varn_runtime::channel::RecvOutcome::Closed => {
                    out.resolve(varn_runtime::channel::next_obj(Value::Null, true));
                }
                varn_runtime::channel::RecvOutcome::Parked(task) => {
                    let out2 = out.clone();
                    task.on_settle(move |res| match res {
                        Ok(v) => {
                            // Re-wrap a composite envelope so `open_resolved`
                            // materializes it into `{value, done:false}`; a
                            // `{value, done}` scalar/close object passes through.
                            let rewrapped = varn_types::value::SendEnvelope::from_value(&v)
                                .map(|env| env.sv.clone());
                            let out_val = match rewrapped {
                                Some(sv) => Value::VmValue(Box::new(
                                    varn_types::value::SendEnvelope { sv, wrap: true },
                                )),
                                None => v,
                            };
                            out2.resolve(out_val);
                        }
                        Err(e) => out2.reject(e),
                    });
                }
            }
            ctx.intern(Value::TaskHandle(out))
        }

        fn receive(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            let out = varn_types::AsyncTask::pending();
            let Some(id) = chan_id(ctx, this) else {
                out.reject(closed_error_obj("channel closed"));
                return ctx.intern(Value::TaskHandle(out));
            };
            match varn_runtime::channel::try_receive(id) {
                varn_runtime::channel::RecvOutcome::Item(v) => {
                    let val_nv = v.to_value_ctx(ctx);
                    let val = ctx.extract(val_nv);
                    out.resolve(val);
                }
                varn_runtime::channel::RecvOutcome::Closed => {
                    out.reject(closed_error_obj("channel closed"));
                }
                varn_runtime::channel::RecvOutcome::Parked(task) => {
                    let out2 = out.clone();
                    task.on_settle(move |res| match res {
                        Ok(v) => {
                            if varn_types::value::SendEnvelope::from_value(&v).is_some() {
                                // Composite delivery (done implied false):
                                // forward the bare envelope; `open_resolved`
                                // materializes the value.
                                out2.resolve(v);
                            } else {
                                // `{value, done}` scalar/close object.
                                let done = matches!(
                                    &v,
                                    Value::Object(o) if matches!(
                                        o.read().inner.get("done")
                                            .map(varn_types::value::nv_to_value),
                                        Some(Value::Bool(true))
                                    )
                                );
                                if done {
                                    out2.reject(closed_error_obj("channel closed"));
                                } else if let Value::Object(o) = &v {
                                    let inner_val = o
                                        .read()
                                        .inner
                                        .get("value")
                                        .map(varn_types::value::nv_to_value)
                                        .unwrap_or(Value::Null);
                                    out2.resolve(inner_val);
                                } else {
                                    // Neither a SendEnvelope nor a `{value, done}`
                                    // object: violates the channel wake-payload
                                    // invariant. Reject loudly instead of leaving
                                    // `out2` (and the awaiting task) pending
                                    // forever.
                                    out2.reject_msg("receive: unexpected wake payload");
                                }
                            }
                        }
                        Err(e) => out2.reject(e),
                    });
                }
            }
            ctx.intern(Value::TaskHandle(out))
        }

        fn dispose(ctx: &mut dyn NativeCtx, this: VmValue) {
            if let Some(id) = chan_id(ctx, this) {
                varn_runtime::channel::close(id);
            }
        }
    }
}

varn_contract! {
    module: "runtime:task",
    class: "Channel",
    contract: "src/modules/std/task/runtime/task_runtime.vn",
    impl ChannelImpl {}
}

varn_contract! {
    module: "runtime:task",
    class: "ChannelClosed",
    extends: "Error",
    contract: "src/modules/std/task/runtime/task_runtime.vn",
    impl ChannelClosedImpl {}
}
