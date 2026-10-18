use crate::exec::ctx::ExecCtx;
use crate::heap::HeapObj;
use crate::value::VmValue;
use varn_types::NativeCtx;

pub(super) fn spawn_isolate(
    ctx: &mut ExecCtx,
    module_path: &str,
    export_name: &str,
    args: Vec<varn_types::value::SendValue>,
) -> Result<varn_types::HostPromise, String> {
    fn worker_error(msg: &str) -> varn_types::value::SendValue {
        varn_types::value::SendValue::Error {
            class: varn_core::RuntimeErrorKind::Error.class_name().to_string(),
            message: msg.to_string(),
        }
    }

    let loader = ctx.loader.clone();
    let parent_caps_inner: varn_types::capabilities::CapabilitySet = (*ctx.capabilities).clone();

    let module_path_str = module_path.to_string();
    let export_name_str = export_name.to_string();

    let settings = ctx.settings;

    let done = varn_types::HostPromise::pending();
    let done_t = done.clone();

    std::thread::Builder::new()
        .stack_size(crate::frame::VM_STACK_BYTES)
        .spawn(move || {
            let mut machine =
                crate::Vm::new(std::rc::Rc::new(rustc_hash::FxHashMap::default()), settings);
            machine.ctx.capabilities = std::rc::Rc::new(parent_caps_inner);
            machine
                .ctx
                .globals_mut()
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

            let mut vm_args = Vec::new();
            for arg in args {
                vm_args.push(arg.to_value_ctx(&mut machine.ctx));
            }

            match machine.ctx.call_vm(func_nv, &vm_args) {
                Ok(res) => {
                    let lazy = if res.is_heap() {
                        match machine.ctx.heap.get(res.as_heap()) {
                            Some(HeapObj::Task(lazy)) => Some(std::rc::Rc::clone(lazy)),
                            Some(
                                HeapObj::Str(_)
                                | HeapObj::Array(_)
                                | HeapObj::Tuple(_)
                                | HeapObj::Object(_)
                                | HeapObj::Record(_)
                                | HeapObj::Buffer(_)
                                | HeapObj::Module(_)
                                | HeapObj::FrozenModule(_)
                                | HeapObj::VmClosure(_)
                                | HeapObj::Class(_)
                                | HeapObj::NativeFn(..)
                                | HeapObj::BoundMethod(_)
                                | HeapObj::Map(_)
                                | HeapObj::Set(_)
                                | HeapObj::TaskHandle(_)
                                | HeapObj::Range(_)
                                | HeapObj::Symbol(_)
                                | HeapObj::EnumVariant(_)
                                | HeapObj::BigInt(_)
                                | HeapObj::Decimal(_)
                                | HeapObj::Char(_)
                                | HeapObj::Generator(_)
                                | HeapObj::Spread(_),
                            )
                            | None => None,
                        }
                    } else {
                        None
                    };
                    if let Some(lazy) = lazy {
                        let cell = machine.ctx.run_lazy_task_sync(lazy);
                        if cell.status() == crate::task::Status::Rejected {
                            let reason = machine.ctx.str_repr(cell.value());
                            done_t.reject(worker_error(&reason));
                            return;
                        }
                    }
                    done_t.resolve(varn_types::value::SendValue::Null);
                }
                Err(e) => done_t.reject(worker_error(&e.to_string())),
            }
        })
        .map_err(|e| format!("failed to spawn isolate thread: {e}"))?;

    Ok(done)
}
