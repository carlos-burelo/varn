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
