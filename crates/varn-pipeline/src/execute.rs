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
    let mut machine = Vm::new(precompiled.clone()).with_loader(loader);
    if std::env::var("VARN_NO_JIT").is_ok() {
        machine.set_no_jit(true);
    }
    machine.set_trace(_debug.trace);

    if _debug.trace {
        varn_utilities::terminal::tagged("pipeline:execute", "starting builtin initialization");
    }

    for builtin_proto in core::core_protos_owned()? {
        if _debug.trace {
            let name = builtin_proto.name.as_deref().unwrap_or("<builtin>");
            varn_utilities::terminal::tagged("pipeline:execute", format_args!("running builtin {name}"));
        }
        let closure = Rc::new(Closure::new(Rc::new(builtin_proto), Vec::new(), Vec::new()));
        machine
            .run(closure)
            .map_err(|e| PipelineError::fatal(format!("failed to run builtin: {}", e)))?;
    }

    // Resolve globals in precompiled modules and entry script to direct array indices
    let mut optimized_precompiled_map = (*precompiled).clone();
    for module_proto_rc in optimized_precompiled_map.values_mut() {
        machine.resolve_globals(Rc::make_mut(module_proto_rc));
    }
    machine.ctx.precompiled = Rc::new(optimized_precompiled_map);

    let mut optimized_proto = proto;
    machine.resolve_globals(&mut optimized_proto);

    let main_closure = Rc::new(Closure::new(Rc::new(optimized_proto), Vec::new(), Vec::new()));

    // Register the main module to support top-level exports in the entry script (needed for Isolates)
    let main_module_id = ModuleId::local_str(&main_closure.proto.chunk.source_file);
    let mut export_map = FxHashMap::default();
    for (idx, name) in main_closure.proto.export_names.iter().enumerate() {
        export_map.insert(name.clone(), idx);
    }
    let mut module_obj = varn_types::ModuleObj::new(main_module_id.clone(), main_closure.proto.export_names.len());
    module_obj.export_map = export_map;
    let module_val = machine.ctx.heap.alloc_module(Rc::new(module_obj));
    machine.ctx.modules.insert(main_module_id, module_val);
    machine.ctx.module_exports.insert(0, module_val);

    if _debug.trace {
        let name = main_closure.proto.name.as_deref().unwrap_or("<main>");
        varn_utilities::terminal::tagged("pipeline:execute", format_args!("running main {name}"));
    }

    loop {
        let res = machine.run(main_closure.clone());
        match res {
            Ok(_) => match machine.ctx.vm_suspend.take() {
                None => break,
                Some(varn_vm::exec::VmSuspend::Await { value, dest_reg }) => {
                    let resolved = match value {
                        varn_types::Value::Task(lazy) => {
                            let handle = machine.ctx.run_lazy_task_sync(lazy.as_ref());
                            match handle.peek_state() {
                                varn_types::task::TaskState::Resolved(v) => v,
                                varn_types::task::TaskState::Rejected(e) => {
                                    return Err(PipelineError::fatal(format!("awaited task failed: {}", e)));
                                }
                                _ => varn_types::Value::Null,
                            }
                        }
                        varn_types::Value::TaskHandle(handle) => match handle.peek_state() {
                            varn_types::task::TaskState::Resolved(v) => v,
                            _ => match varn_vm::exec::ExecCtx::wait_task_handle(handle.clone()) {
                                Ok(v) => v,
                                Err(e) => return Err(PipelineError::fatal(format!("awaited task failed: {}", e))),
                            }
                        },
                        other => other,
                    };
                    let resolved_nv = machine.ctx.heap.intern(resolved);

                    if let Some(frame) = machine.ctx.frames.last() {
                        let base = frame.base;
                        let slot = base + dest_reg as usize;
                        if slot < machine.ctx.stack.len() {
                            machine.ctx.stack[slot] = resolved_nv;
                        }
                    }
                }
                Some(varn_vm::exec::VmSuspend::Task(_task)) => {}
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
