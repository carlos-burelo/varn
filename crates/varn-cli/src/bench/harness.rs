use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustc_hash::FxHashMap;
use varn_compiler::FunctionProto;
use varn_core::ModuleId;
use varn_vm::loader::CompositeLoader;
use varn_vm::Vm;

use crate::error::CliError;

pub struct VmFactory {
    precompiled: Rc<FxHashMap<ModuleId, Rc<FunctionProto>>>,
    builtins: Vec<Rc<FunctionProto>>,
    loader: Arc<CompositeLoader>,
    module_id: ModuleId,
    proto: Rc<FunctionProto>,
}

impl VmFactory {
    pub fn new(
        precompiled: Rc<FxHashMap<ModuleId, Rc<FunctionProto>>>,
        builtins: Vec<FunctionProto>,
        loader: Arc<CompositeLoader>,
        module_id: ModuleId,
        proto: Rc<FunctionProto>,
    ) -> Result<Self, CliError> {
        let factory = Self {
            precompiled,
            builtins: builtins.into_iter().map(Rc::new).collect(),
            loader,
            module_id,
            proto,
        };
        factory.try_build().map_err(CliError::fatal)?;
        Ok(factory)
    }

    pub fn build(&self) -> Vm {
        self.try_build()
            .expect("initialization already succeeded in VmFactory::new")
    }

    fn try_build(&self) -> Result<Vm, String> {
        let silent = varn_builtins::is_print_silent();
        varn_builtins::set_print_silent(true);
        varn_builtins::set_testing_silent(true);
        let mut machine = Vm::new(
            self.precompiled.clone(),
            varn_vm::ExecSettings::from_env(false),
        )
        .with_loader(self.loader.clone());
        let init = self.builtins.iter().try_for_each(|bp| {
            machine
                .run(bp.clone())
                .map(|_| ())
                .map_err(|e| format!("builtin init failed: {e}"))
        });
        if init.is_ok() {
            varn_vm::prefill_native_modules(&mut machine);
            machine.collect_gc();
        }
        varn_builtins::set_print_silent(silent);
        varn_builtins::set_testing_silent(silent);
        init?;

        let mut export_map = FxHashMap::default();
        for (idx, name) in self.proto.export_names.iter().enumerate() {
            export_map.insert(name.clone(), idx);
        }
        let mut module_obj =
            varn_types::ModuleObj::new(self.module_id.clone(), self.proto.export_names.len());
        module_obj.export_map = export_map;
        let module_val = machine.ctx.heap.alloc_module(Rc::new(module_obj));
        unsafe { &mut *machine.ctx.modules.get() }.insert(self.module_id.clone(), module_val);
        machine.ctx.module_exports.insert(0, module_val);

        Ok(machine)
    }

    pub fn entry_proto(&self) -> Rc<FunctionProto> {
        self.proto.clone()
    }

    pub fn run_once(&self) -> Result<Vm, String> {
        let mut machine = self.build();
        run_vm_to_completion(&mut machine, self.entry_proto())?;
        Ok(machine)
    }
}

pub fn run_vm_to_completion(machine: &mut Vm, entry: Rc<FunctionProto>) -> Result<(), String> {
    loop {
        let res = machine.run(entry.clone());
        match res {
            Ok(_) => match machine.ctx.vm_suspend.take() {
                None => break,
                Some(varn_vm::exec::VmSuspend::Await { value, dest_reg }) => {
                    match machine.ctx.settle_awaited(value) {
                        Ok(resolved) => {
                            if let Some(frame) = machine.ctx.frames.last() {
                                let base = frame.base;
                                let _ = machine.ctx.stack.unbox_into_reg(
                                    base,
                                    dest_reg as usize,
                                    resolved,
                                );
                            }
                        }
                        Err(thrown) => {
                            let err = varn_vm::exec::exceptions::build_thrown_error(
                                thrown,
                                &machine.ctx.heap,
                                &machine.ctx.frames,
                            );
                            if let Some(handler) =
                                machine.ctx.try_handlers.pop_if(|h| h.frame_depth > 0)
                            {
                                let thrown_val = err.thrown.unwrap_or(varn_types::VmValue::null());
                                let _ = varn_vm::exec::frame_ctrl::unwind_to_handler(
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
                                return Err(msg);
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
                return Err(msg);
            }
        }
    }
    Ok(())
}

fn compiled_keys() -> std::collections::BTreeSet<(String, usize)> {
    varn_vm::varn_jit::stats::take_records()
        .into_iter()
        .map(|r| (r.name, r.words))
        .collect()
}

pub fn time_n<F: Fn() -> Result<(), String>>(runs: usize, f: F) -> Result<Vec<Duration>, CliError> {
    f().map_err(|e| CliError::fatal(format!("bench warmup failed: {e}")))?;

    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        f().map_err(|e| CliError::fatal(format!("bench run failed: {e}")))?;
        samples.push(start.elapsed());
    }
    Ok(samples)
}

pub fn time_n_freq_setup_progress<T, S, F, P>(
    runs: usize,
    setup: S,
    f: F,
    progress: P,
) -> Result<(Vec<Duration>, Option<crate::cpu_freq::CpuFreq>, u64), CliError>
where
    S: Fn() -> T,
    F: Fn(&mut T) -> Result<(), String>,
    P: Fn(usize, &[Duration]),
{
    varn_vm::varn_jit::stats::start_recording();
    let mut warm = setup();
    f(&mut warm).map_err(|e| CliError::fatal(format!("bench warmup failed: {e}")))?;
    let warmed = compiled_keys();
    varn_vm::varn_jit::stats::start_recording();

    let mut samples = Vec::with_capacity(runs);
    let mut peak = None;
    for i in 0..runs {
        let mut state = setup();
        let start = Instant::now();
        f(&mut state).map_err(|e| CliError::fatal(format!("bench run failed: {e}")))?;
        samples.push(start.elapsed());
        peak = crate::cpu_freq::keep_peak(peak, crate::cpu_freq::sample());
        progress(i + 1, &samples);
    }
    let tiered = compiled_keys().difference(&warmed).count() as u64;
    Ok((samples, peak, tiered))
}

pub fn time_n_freq_setup<T, S, F>(
    runs: usize,
    setup: S,
    f: F,
) -> Result<(Vec<Duration>, Option<crate::cpu_freq::CpuFreq>, u64), CliError>
where
    S: Fn() -> T,
    F: Fn(&mut T) -> Result<(), String>,
{
    varn_vm::varn_jit::stats::start_recording();
    let mut warm = setup();
    f(&mut warm).map_err(|e| CliError::fatal(format!("bench warmup failed: {e}")))?;
    let warmed = compiled_keys();
    varn_vm::varn_jit::stats::start_recording();

    let mut samples = Vec::with_capacity(runs);
    let mut peak = None;
    for _ in 0..runs {
        let mut state = setup();
        let start = Instant::now();
        f(&mut state).map_err(|e| CliError::fatal(format!("bench run failed: {e}")))?;
        samples.push(start.elapsed());
        peak = crate::cpu_freq::keep_peak(peak, crate::cpu_freq::sample());
    }
    let tiered = compiled_keys().difference(&warmed).count() as u64;
    Ok((samples, peak, tiered))
}
