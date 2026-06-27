use super::core;
use crate::error::CliError;
use varn_pipeline::DebugFlags;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_opt::FunctionProto;
use varn_core::ModuleId;
use varn_types::value::Closure;
use varn_utilities::terminal;
use varn_vm::loader::CompositeLoader;
use varn_vm::Vm;

type PipelineResult<T> = Result<T, CliError>;

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
    machine.set_trace(_debug.trace);

    if _debug.trace {
        terminal::tagged("cli:execute", "starting builtin initialization");
    }

    for builtin_proto in core::core_protos_owned()? {
        if _debug.trace {
            let name = builtin_proto.name.as_deref().unwrap_or("<builtin>");
            terminal::tagged("cli:execute", format!("running builtin {}", name));
        }
        let closure = Rc::new(Closure::new(Rc::new(builtin_proto), Vec::new(), Vec::new()));
        machine
            .run(closure)
            .map_err(|e| CliError::fatal(format!("failed to run builtin: {}", e)))?;
    }

    let main_closure = Rc::new(Closure::new(Rc::new(proto), Vec::new(), Vec::new()));
    if _debug.trace {
        terminal::tagged(
            "cli:execute",
            format!("running main {}", main_closure.proto.name.as_deref().unwrap_or("<main>")),
        );
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
                                _ => varn_types::Value::Null,
                            }
                        }
                        varn_types::Value::TaskHandle(handle) => match handle.peek_state() {
                            varn_types::task::TaskState::Resolved(v) => v,
                            _ => match varn_vm::exec::ExecCtx::wait_task_handle(handle.clone()) {
                                Ok(v) => v,
                                Err(e) => return Err(CliError::fatal(format!("awaited task failed: {}", e))),
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
                return Err(CliError::fatal(msg));
            }
        }
    }
    Ok(())
}
