use varn_op_macros::varn_contract;
use varn_types::value::SendValue;
use varn_types::{HostOpen, NativeCtx, VmValue};

pub struct TaskRuntime;

pub struct IsolateHandleImpl;

varn_contract! {
    module: "runtime:task",
    contract: "src/modules/runtime/task/task_runtime.vn",
    impl TaskRuntime {
        fn spawn(ctx: &mut dyn NativeCtx, target: VmValue, args: &[VmValue]) -> Result<VmValue, String> {
            ctx.spawn_vm(target, args)
        }

        fn sleep(ctx: &mut dyn NativeCtx, ms: i64) -> Result<VmValue, String> {
            Ok(ctx.suspend_timer(ms.max(0) as u64))
        }

        fn parallel(ctx: &mut dyn NativeCtx, tasks: VmValue) -> Result<VmValue, String> {
            ctx.task_gather(tasks)
        }

        fn cancelTask(ctx: &mut dyn NativeCtx, handle_nv: VmValue) -> Result<(), String> {
            ctx.task_cancel(handle_nv)?;
            varn_runtime::timer::note_cancel();
            Ok(())
        }

        fn yieldTask(ctx: &mut dyn NativeCtx) -> Result<VmValue, String> {
            Ok(ctx.task_yield())
        }

        fn spawnIsolate(ctx: &mut dyn NativeCtx, func: VmValue, args: VmValue) -> Result<VmValue, String> {
            let (resolved_path, export_name) = ctx
                .get_function_location(func)
                .ok_or_else(|| "spawnIsolate: first argument must be a function reference".to_string())?;

            let module_val = match ctx.load_module(&resolved_path) {
                Ok(m) => m,
                Err(e) => {
                    let err_msg = format!("spawnIsolate: failed to load module '{resolved_path}': {e}");
                    return Ok(reject_with_message(ctx, &err_msg));
                }
            };

            let exported_fn = ctx.get_field(module_val, &export_name);
            if exported_fn != Some(func) {
                let err_msg = if export_name.starts_with('<') {
                    "spawnIsolate: first argument must be a function reference, not an anonymous closure".to_string()
                } else {
                    format!("spawnIsolate: function '{export_name}' is not a top-level exported function of module '{resolved_path}'")
                };
                return Ok(reject_with_message(ctx, &err_msg));
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

            let done = ctx.spawn_isolate(&resolved_path, &export_name, worker_args)?;
            let handle_nv = ctx
                .alloc_instance("IsolateHandle")
                .ok_or("spawnIsolate: IsolateHandle class not registered")?;
            let done_nv = ctx.task_from_host(done, HostOpen::Plain);
            ctx.set_field(handle_nv, "_done", done_nv);
            Ok(handle_nv)
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
    class: "IsolateHandle",
    contract: "src/modules/runtime/task/task_runtime.vn",
    impl IsolateHandleImpl {
        // `join()` yields the worker's join task (resolves `Null` on normal
        // completion, rejects a typed `Error` if the worker threw). `_done`
        // holds that task, so `await handle.join()` drives it.
        fn join(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            ctx.get_field(this, "_done").unwrap_or(VmValue::null())
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
        let iter_nv = ctx.alloc_bound_native(nv, receiver_self_iterator, "[Symbol.asyncIterator]");
        ctx.set_field(nv, "Symbol.asyncIterator", iter_nv);
    }
    Ok(nv)
}

fn receiver_self_iterator(
    _ctx: &mut dyn NativeCtx,
    args: &[VmValue],
) -> varn_types::NativeFnResult {
    args.first()
        .copied()
        .ok_or_else(|| "receiver iterator: missing self".into())
}

fn chan_id(ctx: &mut dyn NativeCtx, this: VmValue) -> Option<u64> {
    let nv = ctx.get_field(this, "_chan")?;
    nv.is_int().then(|| nv.as_int() as u64)
}

fn reject_with_message(ctx: &mut dyn NativeCtx, message: &str) -> VmValue {
    let message_nv = ctx.alloc_str(message);
    let obj = ctx.alloc_object();
    ctx.set_field(obj, "message", message_nv);
    ctx.task_rejected(obj)
}

fn reject_closed(ctx: &mut dyn NativeCtx) -> VmValue {
    let error = SendValue::Error {
        class: "ChannelClosed".to_string(),
        message: "channel closed".to_string(),
    }
    .to_value_ctx(ctx);
    ctx.task_rejected(error)
}

pub fn mint_endpoint_marker(ctx: &mut dyn NativeCtx, nv: VmValue) -> VmValue {
    let Some(dir_nv) = ctx.get_field(nv, "__chanEndpoint") else {
        return nv;
    };
    let Some(id_nv) = ctx.get_field(nv, "__chanId") else {
        return nv;
    };
    if !id_nv.is_int() {
        return nv;
    }
    let class_name = if ctx.str_owned(dir_nv).as_deref() == Some("tx") {
        "Sender"
    } else {
        "Receiver"
    };
    alloc_endpoint(ctx, class_name, id_nv.as_int() as u64).unwrap_or(nv)
}

pub fn open_sent(ctx: &mut dyn NativeCtx, value: &SendValue) -> VmValue {
    let nv = value.to_value_ctx(ctx);
    mint_endpoint_marker(ctx, nv)
}

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
    contract: "src/modules/runtime/task/task_runtime.vn",
    impl SenderImpl {
        fn send(ctx: &mut dyn NativeCtx, this: VmValue, msg: VmValue) -> VmValue {
            let Some(id) = chan_id(ctx, this) else {
                return reject_closed(ctx);
            };
            let send_val = match ctx.to_sendable(msg) {
                Ok(v) => v,
                Err(e) => return reject_with_message(ctx, &format!("send: {e}")),
            };
            match varn_runtime::channel::send(id, send_val) {
                varn_runtime::channel::SendOutcome::Sent => {
                    let null = ctx.null_val();
                    ctx.task_resolved(null)
                }
                varn_runtime::channel::SendOutcome::Closed => reject_closed(ctx),
                varn_runtime::channel::SendOutcome::Parked(promise) => {
                    ctx.task_from_host(promise, HostOpen::SendAck)
                }
            }
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
    contract: "src/modules/runtime/task/task_runtime.vn",
    impl ReceiverImpl {
        fn next(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            let finished = |ctx: &mut dyn NativeCtx| {
                let null = ctx.null_val();
                let result = next_result(ctx, null, true);
                ctx.task_resolved(result)
            };
            let Some(id) = chan_id(ctx, this) else {
                return finished(ctx);
            };
            match varn_runtime::channel::try_receive(id) {
                varn_runtime::channel::RecvOutcome::Item(item) => {
                    let value = open_sent(ctx, &item);
                    let result = next_result(ctx, value, false);
                    ctx.task_resolved(result)
                }
                varn_runtime::channel::RecvOutcome::Closed => finished(ctx),
                varn_runtime::channel::RecvOutcome::Parked(promise) => {
                    ctx.task_from_host(promise, HostOpen::ReceiveNext)
                }
            }
        }

        fn receive(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            let Some(id) = chan_id(ctx, this) else {
                return reject_closed(ctx);
            };
            match varn_runtime::channel::try_receive(id) {
                varn_runtime::channel::RecvOutcome::Item(item) => {
                    let value = open_sent(ctx, &item);
                    ctx.task_resolved(value)
                }
                varn_runtime::channel::RecvOutcome::Closed => reject_closed(ctx),
                varn_runtime::channel::RecvOutcome::Parked(promise) => {
                    ctx.task_from_host(promise, HostOpen::Receive)
                }
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
    class: "Channel",
    contract: "src/modules/runtime/task/task_runtime.vn",
    impl ChannelImpl {}
}

varn_contract! {
    module: "runtime:task",
    class: "ChannelClosed",
    extends: "Error",
    contract: "src/modules/runtime/task/task_runtime.vn",
    impl ChannelClosedImpl {}
}
