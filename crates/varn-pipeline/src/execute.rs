use crate::core;
use crate::PipelineError;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_compiler::FunctionProto;
use varn_core::ModuleId;
use varn_debug_flags::DebugFlags;
use varn_types::capabilities::CapabilitySet;
use varn_vm::Vm;

type PipelineResult<T> = Result<T, PipelineError>;

pub fn execute(
    proto: FunctionProto,
    precompiled: Rc<FxHashMap<ModuleId, Rc<FunctionProto>>>,
    source: &str,
    path: &str,
    debug: &DebugFlags,
) -> PipelineResult<()> {
    execute_with_caps(
        proto,
        precompiled,
        source,
        path,
        debug,
        CapabilitySet::allow_all(),
    )
}

pub fn execute_with_caps(
    proto: FunctionProto,
    precompiled: Rc<FxHashMap<ModuleId, Rc<FunctionProto>>>,
    _source: &str,
    _path: &str,
    _debug: &DebugFlags,
    capabilities: CapabilitySet,
) -> PipelineResult<()> {
    let mut machine = boot_machine(precompiled, capabilities, _debug.trace)?;
    let main_proto = enter_main(&mut machine, proto);
    if _debug.trace {
        varn_core::term::terminal::tagged(
            "pipeline:execute",
            format_args!(
                "running main {}",
                main_proto.name.as_deref().unwrap_or("<main>")
            ),
        );
    }
    let mut resume = |_: &mut Vm| varn_vm::debug::BreakAction::Resume;
    match varn_vm::debug::drive_main(&mut machine, &main_proto, &mut resume) {
        varn_vm::debug::DriveResult::Done(_) => {}
        varn_vm::debug::DriveResult::Stopped => {}
        varn_vm::debug::DriveResult::Failed(e) => {
            let msg = format_runtime_error(None, &e.message, &e.frames);
            return Err(PipelineError::fatal(msg));
        }
    }
    if _debug.gc {
        eprintln!("{}", machine.gc_report());
    }
    Ok(())
}

pub fn boot_machine(
    precompiled: Rc<FxHashMap<ModuleId, Rc<FunctionProto>>>,
    capabilities: CapabilitySet,
    trace: bool,
) -> PipelineResult<Vm> {
    let loader = std::sync::Arc::new(crate::stdlib_loader::PipelineLoader::new());
    let settings = varn_vm::ExecSettings::from_env(trace);
    let mut machine = Vm::new(precompiled.clone(), settings).with_loader(loader);
    machine.ctx.capabilities = Rc::new(capabilities);
    varn_vm::prefill_native_modules(&mut machine);

    if trace {
        varn_core::term::terminal::tagged("pipeline:execute", "starting builtin initialization");
    }

    for builtin_proto in core::core_protos_owned()? {
        if trace {
            let name = builtin_proto.name.as_deref().unwrap_or("<builtin>");
            varn_core::term::terminal::tagged(
                "pipeline:execute",
                format_args!("running builtin {name}"),
            );
        }
        let closure = Rc::new(builtin_proto);
        machine
            .run(closure)
            .map_err(|e| PipelineError::fatal(format!("failed to run builtin: {}", e)))?;
    }
    Ok(machine)
}

pub fn enter_main(machine: &mut Vm, proto: FunctionProto) -> Rc<FunctionProto> {
    let main_proto = Rc::new(proto);

    let main_module_id = ModuleId::local_str(&main_proto.chunk.source_file);
    let mut export_map = FxHashMap::default();
    for (idx, name) in main_proto.export_names.iter().enumerate() {
        export_map.insert(name.clone(), idx);
    }
    let mut module_obj =
        varn_types::ModuleObj::new(main_module_id.clone(), main_proto.export_names.len());
    module_obj.export_map = export_map;
    let module_val = machine.ctx.heap.alloc_module(Rc::new(module_obj));
    unsafe { &mut *machine.ctx.modules.get() }.insert(main_module_id, module_val);
    machine.ctx.module_exports.insert(0, module_val);
    main_proto
}

fn format_runtime_error(
    prefix: Option<&str>,
    message: &str,
    frames: &[varn_vm::FrameInfo],
) -> String {
    let mut msg = match prefix {
        Some(p) => format!("{p}: {message}"),
        None => message.to_string(),
    };
    let cwd = std::env::current_dir().ok();
    for frame in frames {
        let mut file_display = frame.file.as_str();
        if file_display.starts_with(r"\\?\") {
            file_display = &file_display[4..];
        }
        let clean_path = if let Some(ref root) = cwd {
            let p = std::path::Path::new(file_display);
            if let Ok(rel) = p.strip_prefix(root) {
                rel.to_string_lossy().replace('\\', "/")
            } else {
                file_display.replace('\\', "/")
            }
        } else {
            file_display.replace('\\', "/")
        };
        msg.push_str(&format!(
            "\n    at {} ({}:{})",
            frame.fn_name, clean_path, frame.line
        ));
    }
    msg
}
