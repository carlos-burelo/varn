use crate::core;
use crate::{Executable, Failure, Policy, RuntimeFailure, Session};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_compiler::FunctionProto;
use varn_core::ModuleId;
use varn_types::capabilities::CapabilitySet;
use varn_types::TestEntry;
use varn_vm::Vm;

type PipelineResult<T> = Result<T, Failure>;

pub enum Unit<'a> {
    Module,
    Test(&'a TestEntry),
}

pub fn execute(session: &Session, executable: Executable, policy: Policy) -> Result<(), Failure> {
    execute_unit(session, &executable, policy, Unit::Module)
}

pub fn execute_unit(
    session: &Session,
    executable: &Executable,
    policy: Policy,
    unit: Unit<'_>,
) -> Result<(), Failure> {
    let mut machine = boot_machine(
        session,
        Rc::clone(&executable.precompiled),
        policy.capabilities,
        policy.trace,
    )?;
    machine.ctx.print_silent = policy.silent;
    let main_proto = enter_main(&mut machine, Rc::new(executable.entry_proto.clone()));
    if policy.trace {
        varn_core::term::terminal::tagged(
            "pipeline:execute",
            format_args!(
                "running main {}",
                main_proto.name.as_deref().unwrap_or("<main>")
            ),
        );
    }
    if let Err(failure) = drive_entry(&mut machine, &main_proto) {
        machine.abandon_heap_at_exit();
        return Err(failure);
    }
    if let Unit::Test(test) = unit
        && let Err(e) = machine.call_main_global(test.global)
    {
        machine.abandon_heap_at_exit();
        return Err(Failure::Runtime(RuntimeFailure {
            message: e.message,
            frames: e.frames,
        }));
    }
    if policy.gc {
        eprintln!("{}", machine.gc_report());
    }
    machine.abandon_heap_at_exit();
    Ok(())
}

pub fn drive_entry(machine: &mut Vm, main_proto: &Rc<FunctionProto>) -> Result<(), Failure> {
    let mut resume = |_: &mut Vm| varn_vm::debug::BreakAction::Resume;
    match varn_vm::debug::drive_main(machine, main_proto, &mut resume) {
        varn_vm::debug::DriveResult::Done(_) | varn_vm::debug::DriveResult::Stopped => Ok(()),
        varn_vm::debug::DriveResult::Failed(e) => Err(Failure::Runtime(RuntimeFailure {
            message: e.message,
            frames: e.frames,
        })),
    }
}

pub fn boot_machine(
    session: &Session,
    precompiled: Rc<FxHashMap<ModuleId, Rc<FunctionProto>>>,
    capabilities: CapabilitySet,
    trace: bool,
) -> PipelineResult<Vm> {
    let loader = std::sync::Arc::new(crate::stdlib_loader::PipelineLoader::new(
        session.shared_resolver(),
    ));
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
            .map_err(|e| Failure::internal(format!("failed to run builtin: {}", e)))?;
    }
    Ok(machine)
}

pub fn enter_main(machine: &mut Vm, main_proto: Rc<FunctionProto>) -> Rc<FunctionProto> {
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
