use varn_op_macros::varn_module;
use varn_types::{NativeCtx, VmValue};

#[varn_module("runtime:task")]
pub(crate) mod dispatch {
    #[allow(unused_imports)]
    use super::*;
    use varn_types::Value;

    #[varn_fn("taskSpawn", cap = "async")]
    pub fn spawn(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let first = args
            .first()
            .copied()
            .ok_or("task.spawn: missing function")?;
        ctx.spawn_vm(first, if args.len() > 1 { &args[1..] } else { &[] })
    }

    #[varn_fn("taskSleep", cap = "async")]
    pub fn sleep(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let ms = match args.first() {
            Some(v) if v.is_int() => v.as_int() as u64,
            _ => 0,
        };
        Ok(ctx.suspend_timer(ms))
    }

    #[varn_fn("taskParallel", cap = "async")]
    pub fn parallel(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let array_nv = args.first().copied().ok_or("parallel: missing array")?;
        if !ctx.is_array(array_nv) {
            return Err("parallel: argument must be an array".to_string());
        }

        let len = ctx.array_len(array_nv);
        let mut handles = Vec::with_capacity(len);

        for i in 0..len {
            if let Some(item_nv) = ctx.array_get(array_nv, i) {
                let handle_nv = ctx.spawn_vm(item_nv, &[])?;
                handles.push(handle_nv);
            }
        }

        use std::sync::{Arc, Mutex};
        let output = varn_types::AsyncTask::pending();
        let pending_count = Arc::new(Mutex::new(handles.len()));
        let results = Arc::new(Mutex::new(vec![varn_types::Value::Null; handles.len()]));

        for (idx, handle_nv) in handles.iter().copied().enumerate() {
            let handle_val = ctx.extract(handle_nv);
            if let varn_types::Value::TaskHandle(handle) = handle_val {
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
                        output_clone.resolve(varn_types::Value::Array(arr));
                    }
                });
            } else {
                let mut count_guard = pending_count.lock().unwrap();
                *count_guard -= 1;
                if *count_guard == 0 {
                    let results_guard = results.lock().unwrap();
                    let arr = varn_types::value::ArrayRef::new(results_guard.clone());
                    output.resolve(varn_types::Value::Array(arr));
                }
            }
        }

        if handles.is_empty() {
            let arr = varn_types::value::ArrayRef::new(Vec::new());
            output.resolve(varn_types::Value::Array(arr));
        }

        Ok(ctx.intern(varn_types::Value::TaskHandle(output)))
    }

    #[varn_fn("spawnIsolate", cap = "async")]
    pub fn spawn_isolate(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let first = args.first().copied().ok_or("spawnIsolate: missing function reference")?;
        
        let (resolved_path, export_name) = ctx.get_function_location(first)
            .ok_or_else(|| "spawnIsolate: first argument must be a function reference".to_string())?;

        let module_val = match ctx.load_module(&resolved_path) {
            Ok(m) => m,
            Err(e) => {
                let async_task = varn_types::AsyncTask::pending();
                let mut obj = varn_types::value::ObjData::new();
                let err_msg = format!("spawnIsolate: failed to load module '{}': {}", resolved_path, e);
                let err_msg_nv = ctx.alloc_str(&err_msg);
                let err_msg_val = varn_types::Value::VmValue(Box::new(varn_types::VmValueRef(err_msg_nv)));
                obj.set_field(std::rc::Rc::from("message"), err_msg_val);
                async_task.reject(varn_types::value::new_object(obj));
                return Ok(ctx.intern(varn_types::Value::TaskHandle(async_task)));
            }
        };

        let exported_fn = ctx.get_field(module_val, &export_name);
        if exported_fn != Some(first) {
            let async_task = varn_types::AsyncTask::pending();
            let mut obj = varn_types::value::ObjData::new();
            let err_msg = if export_name.starts_with('<') {
                "spawnIsolate: first argument must be a function reference, not an anonymous closure".to_string()
            } else {
                format!("spawnIsolate: function '{}' is not a top-level exported function of module '{}'", export_name, resolved_path)
            };
            let err_msg_nv = ctx.alloc_str(&err_msg);
            let err_msg_val = varn_types::Value::VmValue(Box::new(varn_types::VmValueRef(err_msg_nv)));
            obj.set_field(std::rc::Rc::from("message"), err_msg_val);
            async_task.reject(varn_types::value::new_object(obj));
            return Ok(ctx.intern(varn_types::Value::TaskHandle(async_task)));
        }

        let args_nv = args.get(1).copied().ok_or("spawnIsolate: missing arguments array")?;
        if !ctx.is_array(args_nv) {
            return Err("spawnIsolate: arguments must be an array".to_string());
        }
        
        let mut worker_args = Vec::new();
        let len = ctx.array_len(args_nv);
        for i in 0..len {
            if let Some(item_nv) = ctx.array_get(args_nv, i) {
                worker_args.push(ctx.to_sendable(item_nv)?);
            }
        }

        let (port_parent, port_child) = varn_runtime::isolate::IsolatePort::new();

        ctx.spawn_isolate(&resolved_path, &export_name, worker_args, Box::new(port_child))?;

        let instance_nv = ctx.alloc_instance("IsolatePort")
            .ok_or("Failed to instantiate IsolatePort")?;
        
        let port_value = Value::VmValue(Box::new(port_parent));
        let port_nv = ctx.intern(port_value);
        ctx.set_field(instance_nv, "_port", port_nv);

        Ok(instance_nv)
    }

    #[varn_class("IsolatePort")]
    pub mod isolate_port_class {
        use super::*;

        #[varn_constructor]
        pub fn constructor(
            _ctx: &mut dyn NativeCtx,
            _this: VmValue,
            _args: &[VmValue],
        ) -> Result<(), String> {
            Ok(())
        }

        #[varn_method("send")]
        pub fn send(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            let msg = args.first().copied().ok_or("IsolatePort.send: missing message")?;
            let send_val = ctx.to_sendable(msg)?;
            
            let port_nv = ctx.get_field(this, "_port").ok_or("IsolatePort: internal port not initialized")?;
            let port_val = ctx.extract(port_nv);
            if let Value::VmValue(payload) = port_val {
                if let Some(port) = payload.as_any().downcast_ref::<varn_runtime::isolate::IsolatePort>() {
                    port.send(send_val)?;
                    return Ok(VmValue::null());
                }
            }
            Err("IsolatePort.send: invalid internal port".to_string())
        }

        #[varn_method("receive")]
        pub fn receive(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            let port_nv = ctx.get_field(this, "_port").ok_or("IsolatePort: internal port not initialized")?;
            let port_val = ctx.extract(port_nv);
            if let Value::VmValue(payload) = port_val {
                if let Some(port) = payload.as_any().downcast_ref::<varn_runtime::isolate::IsolatePort>() {
                    let port_clone = port.clone();
                    let async_task = varn_types::AsyncTask::pending();
                    // Directly perform blocking receive on the current thread
                    if let Some(send_val) = port_clone.receive_blocking() {
                        let val_nv = send_val.to_value_ctx(ctx);
                        let val = ctx.extract(val_nv);
                        async_task.resolve(val);
                    } else {
                        async_task.resolve(Value::Null);
                    }
                    return Ok(ctx.intern(Value::TaskHandle(async_task)));
                }
            }
            Err("IsolatePort.receive: invalid internal port".to_string())
        }
    }
}
