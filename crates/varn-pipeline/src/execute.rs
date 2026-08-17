use crate::core;
use crate::PipelineError;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_compiler::FunctionProto;
use varn_core::ModuleId;
use varn_debug::flags::DebugFlags;
use varn_types::value::Closure;
use varn_vm::loader::CompositeLoader;
use varn_vm::Vm;

type PipelineResult<T> = Result<T, PipelineError>;

pub fn execute(
    proto: FunctionProto,
    precompiled: Rc<FxHashMap<ModuleId, Rc<FunctionProto>>>,
    _source: &str,
    _path: &str,
    _debug: &DebugFlags,
) -> PipelineResult<()> {
    let loader = std::sync::Arc::new(CompositeLoader::new(vec![
        Box::new(crate::stdlib_loader::FileLoader),
        Box::new(crate::stdlib_loader::StdlibLoader),
    ]));
    let settings = varn_vm::ExecSettings::from_env(_debug.trace);
    let mut machine = Vm::new(precompiled.clone(), settings).with_loader(loader);
    varn_vm::prefill_native_modules(&mut machine);

    if _debug.trace {
        varn_term::terminal::tagged("pipeline:execute", "starting builtin initialization");
    }

    for builtin_proto in core::core_protos_owned()? {
        if _debug.trace {
            let name = builtin_proto.name.as_deref().unwrap_or("<builtin>");
            varn_term::terminal::tagged("pipeline:execute", format_args!("running builtin {name}"));
        }
        let closure = Rc::new(Closure::new(Rc::new(builtin_proto), Vec::new(), Vec::new()));
        machine
            .run(closure)
            .map_err(|e| PipelineError::fatal(format!("failed to run builtin: {}", e)))?;
    }

    // Pre-binding the module map to THIS VM's store is an optimisation, not the
    // contract: `eval_module_proto` re-resolves any proto whose recorded store
    // id does not match the VM evaluating it, which is what keeps isolate
    // workers (their own store) correct. Doing it here means the main VM's
    // modules — re-evaluated once per run by the bench harness — hit that
    // check instead of rewriting themselves every iteration.
    let mut optimized_precompiled_map = (*precompiled).clone();
    for module_proto_rc in optimized_precompiled_map.values_mut() {
        machine.resolve_globals(Rc::make_mut(module_proto_rc));
    }
    machine.ctx.precompiled = Rc::new(optimized_precompiled_map);

    let mut optimized_proto = proto;
    machine.resolve_globals(&mut optimized_proto);

    let main_closure = Rc::new(Closure::new(
        Rc::new(optimized_proto),
        Vec::new(),
        Vec::new(),
    ));

    let main_module_id = ModuleId::local_str(&main_closure.proto.chunk.source_file);
    let mut export_map = FxHashMap::default();
    for (idx, name) in main_closure.proto.export_names.iter().enumerate() {
        export_map.insert(name.clone(), idx);
    }
    let mut module_obj = varn_types::ModuleObj::new(
        main_module_id.clone(),
        main_closure.proto.export_names.len(),
    );
    module_obj.export_map = export_map;
    let module_val = machine.ctx.heap.alloc_module(Rc::new(module_obj));
    machine.ctx.modules.insert(main_module_id, module_val);
    machine.ctx.module_exports.insert(0, module_val);

    if _debug.trace {
        let name = main_closure.proto.name.as_deref().unwrap_or("<main>");
        varn_term::terminal::tagged("pipeline:execute", format_args!("running main {name}"));
    }

    loop {
        let res = machine.run(main_closure.clone());
        match res {
            Ok(_) => match machine.ctx.vm_suspend.take() {
                None => break,
                Some(varn_vm::exec::VmSuspend::Await { value, dest_reg }) => {
                    let res_val = match value {
                        varn_types::Value::Task(lazy) => {
                            let handle = machine.ctx.run_lazy_task_sync(lazy.as_ref());
                            match handle.peek_state() {
                                varn_types::task::TaskState::Resolved(v) => Ok(v),
                                varn_types::task::TaskState::Rejected(e) => Err(e),
                                _ => Ok(varn_types::Value::Null),
                            }
                        }
                        varn_types::Value::TaskHandle(handle) => match handle.peek_state() {
                            varn_types::task::TaskState::Resolved(v) => Ok(v),
                            varn_types::task::TaskState::Rejected(e) => Err(e),
                            _ => varn_vm::exec::ExecCtx::wait_task_handle_value(handle.clone()),
                        },
                        other => Ok(other),
                    };

                    match res_val {
                        Ok(resolved) => {
                            let resolved = varn_vm::exec::host_values::open_resolved(
                                &mut machine.ctx,
                                resolved,
                            );
                            let resolved_nv = machine.ctx.heap.intern(resolved);

                            if let Some(frame) = machine.ctx.frames.last() {
                                let base = frame.base;
                                let slot = base + dest_reg as usize;
                                if slot < machine.ctx.stack.len() {
                                    machine.ctx.stack[slot] = resolved_nv;
                                }
                            }
                        }
                        Err(thrown) => {
                            let thrown =
                                varn_vm::exec::host_values::open_rejected(&mut machine.ctx, thrown);
                            let thrown_nv = machine.ctx.heap.intern(thrown.clone());
                            let err = varn_vm::exec::exceptions::build_thrown_error(
                                thrown_nv,
                                &machine.ctx.heap,
                                &machine.ctx.frames,
                            );
                            if let Some(handler) = machine.ctx.try_handlers.pop() {
                                let thrown_val = err.thrown.unwrap_or(varn_types::VmValue::null());
                                varn_vm::exec::frame_ctrl::unwind_to_handler(
                                    &mut machine.ctx,
                                    handler,
                                    thrown_val,
                                );
                            } else {
                                let mut msg = format!("awaited task failed: {}", err.message);
                                for frame in &err.frames {
                                    msg.push_str(&format!(
                                        "\n  at {} ({}:{})",
                                        frame.fn_name, frame.file, frame.line
                                    ));
                                }
                                return Err(PipelineError::fatal(msg));
                            }
                        }
                    }
                }
                Some(varn_vm::exec::VmSuspend::Yield { .. }) => {}
            },
            Err(e) => {
                let mut msg = format!("runtime error: {}", e.message);
                for frame in &e.frames {
                    msg.push_str(&format!(
                        "\n  at {} ({}:{})",
                        frame.fn_name, frame.file, frame.line
                    ));
                }
                return Err(PipelineError::fatal(msg));
            }
        }
    }
    Ok(())
}
