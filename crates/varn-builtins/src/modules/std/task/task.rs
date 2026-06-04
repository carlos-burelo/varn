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
        let module_path_nv = args.first().copied().ok_or("spawnIsolate: missing module path")?;
        let module_path = ctx.str_owned(module_path_nv).ok_or("spawnIsolate: module path must be a string")?;
        
        let export_name_nv = args.get(1).copied().ok_or("spawnIsolate: missing export name")?;
        let export_name = ctx.str_owned(export_name_nv).ok_or("spawnIsolate: export name must be a string")?;
        
        let args_nv = args.get(2).copied().ok_or("spawnIsolate: missing arguments")?;
        if !ctx.is_array(args_nv) {
            return Err("spawnIsolate: arguments must be an array".to_string());
        }
        
        let mut worker_args = Vec::new();
        let len = ctx.array_len(args_nv);
        for i in 0..len {
            if let Some(item_nv) = ctx.array_get(args_nv, i) {
                worker_args.push(ctx.extract(item_nv).to_sendable()?);
            }
        }

        let current_file = ctx.current_source_file().unwrap_or_default();
        let resolved_path = if module_path.starts_with('.') {
            let parent_dir = std::path::Path::new(&current_file).parent().unwrap_or(std::path::Path::new("."));
            parent_dir.join(&module_path).to_string_lossy().into_owned()
        } else {
            module_path
        };

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
            let send_val = ctx.extract(msg).to_sendable()?;
            
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
                        let val = send_val.to_value();
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
