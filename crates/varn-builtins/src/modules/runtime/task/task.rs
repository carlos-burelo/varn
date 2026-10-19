use varn_macros::varn_contract;
use varn_types::value::{endpoint_id, ChannelEndpoint, SendValue};
use varn_types::{HostOpen, NativeCtx, VmValue};

pub struct TaskRuntime;

pub struct IsolateHandleImpl;

pub struct SenderImpl;
pub struct ReceiverImpl;
pub struct ChannelImpl;
pub struct ChannelClosedImpl;

fn reject_with_message(ctx: &mut dyn NativeCtx, message: &str) -> VmValue {
    let message_nv = ctx.alloc_str(message);
    let obj = ctx.alloc_object();
    ctx.set_field(obj, varn_core::MemberKey::Message.as_str(), message_nv);
    ctx.task_rejected(obj)
}

fn reject_closed(ctx: &mut dyn NativeCtx) -> VmValue {
    let error = SendValue::Error {
        class: varn_types::value::CHANNEL_CLOSED_CLASS.to_string(),
        message: "channel closed".to_string(),
    }
    .to_value_ctx(ctx);
    ctx.task_rejected(error)
}

fn next_result(ctx: &mut dyn NativeCtx, value_nv: VmValue, done: bool) -> VmValue {
    ctx.alloc_object_with_shape(
        &varn_types::iter_result_shape(),
        vec![value_nv, VmValue::from_bool(done)],
    )
}

varn_macros::varn_record! {
    contract: "src/modules/runtime/task/task_runtime.vn",
    class: "IsolateHandle",
    struct IsolateHandleRecord;
}

varn_macros::varn_record! {
    contract: "src/modules/runtime/task/task_runtime.vn",
    class: "Channel",
    struct ChannelRecord;
}

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
            crate::runtime::timer::note_cancel();
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
            let done = ctx.task_from_host(done, HostOpen::Plain);
            IsolateHandleRecord { done }.alloc(ctx)
        }

        fn channel(ctx: &mut dyn NativeCtx, capacity: i64) -> Result<VmValue, String> {
            if capacity < 1 {
                return Err("channel: capacity must be >= 1".to_string());
            }
            let id = crate::runtime::channel::create(capacity as usize);
            let tx = ChannelEndpoint::Sender.alloc(ctx, id)?;
            let rx = ChannelEndpoint::Receiver.alloc(ctx, id)?;
            ChannelRecord { tx, rx }.alloc(ctx)
        }
    }

    class: "IsolateHandle",
    impl IsolateHandleImpl {



        fn join(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            ctx.get_field(this, IsolateHandleRecord::DONE)
                .unwrap_or(VmValue::null())
        }
    }

    class: "Sender",
    impl SenderImpl {
        fn send(ctx: &mut dyn NativeCtx, this: VmValue, msg: VmValue) -> VmValue {
            let Some(id) = endpoint_id(ctx, this) else {
                return reject_closed(ctx);
            };
            let send_val = match ctx.to_sendable(msg) {
                Ok(v) => v,
                Err(e) => return reject_with_message(ctx, &format!("send: {e}")),
            };
            match crate::runtime::channel::send(id, send_val) {
                crate::runtime::channel::SendOutcome::Sent => {
                    let null = ctx.null_val();
                    ctx.task_resolved(null)
                }
                crate::runtime::channel::SendOutcome::Closed => reject_closed(ctx),
                crate::runtime::channel::SendOutcome::Parked(promise) => {
                    ctx.task_from_host(promise, HostOpen::SendAck)
                }
            }
        }

        fn close(ctx: &mut dyn NativeCtx, this: VmValue) {
            if let Some(id) = endpoint_id(ctx, this) {
                crate::runtime::channel::close(id);
            }
        }

        fn dispose(ctx: &mut dyn NativeCtx, this: VmValue) {
            if let Some(id) = endpoint_id(ctx, this) {
                crate::runtime::channel::close(id);
            }
        }
    }

    class: "Receiver",
    impl ReceiverImpl {
        fn next(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            let finished = |ctx: &mut dyn NativeCtx| {
                let null = ctx.null_val();
                let result = next_result(ctx, null, true);
                ctx.task_resolved(result)
            };
            let Some(id) = endpoint_id(ctx, this) else {
                return finished(ctx);
            };
            match crate::runtime::channel::try_receive(id) {
                crate::runtime::channel::RecvOutcome::Item(item) => {
                    let value = item.to_value_ctx(ctx);
                    let result = next_result(ctx, value, false);
                    ctx.task_resolved(result)
                }
                crate::runtime::channel::RecvOutcome::Closed => finished(ctx),
                crate::runtime::channel::RecvOutcome::Parked(promise) => {
                    ctx.task_from_host(promise, HostOpen::ReceiveNext)
                }
            }
        }

        fn receive(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            let Some(id) = endpoint_id(ctx, this) else {
                return reject_closed(ctx);
            };
            match crate::runtime::channel::try_receive(id) {
                crate::runtime::channel::RecvOutcome::Item(item) => {
                    let value = item.to_value_ctx(ctx);
                    ctx.task_resolved(value)
                }
                crate::runtime::channel::RecvOutcome::Closed => reject_closed(ctx),
                crate::runtime::channel::RecvOutcome::Parked(promise) => {
                    ctx.task_from_host(promise, HostOpen::Receive)
                }
            }
        }

        fn dispose(ctx: &mut dyn NativeCtx, this: VmValue) {
            if let Some(id) = endpoint_id(ctx, this) {
                crate::runtime::channel::close(id);
            }
        }
    }

    class: "Channel",
    impl ChannelImpl {}

    class: "ChannelClosed",
    impl ChannelClosedImpl {}
}
