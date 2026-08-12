//! Sample collection and the VM construction shared by every measured run.

use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustc_hash::FxHashMap;
use varn_core::ModuleId;
use varn_opt::FunctionProto;
use varn_types::value::Closure;
use varn_vm::loader::CompositeLoader;
use varn_vm::Vm;

use crate::error::CliError;

/// Everything needed to stand up an identical VM for one run.
///
/// The four call sites that used to build this inline (timed run, e2e run, and
/// two profiling runs) drifted apart in exactly the way that makes a benchmark
/// lie: one of them forgot `resolve_globals`, so it measured an
/// unresolved-globals interpreter path no user ever hits.
pub struct VmFactory {
    pub globals: varn_vm::GlobalStore,
    pub heap: varn_vm::Heap,
    pub precompiled: Rc<FxHashMap<ModuleId, Rc<FunctionProto>>>,
    pub modules: FxHashMap<ModuleId, varn_types::VmValue>,
    pub loader: Arc<CompositeLoader>,
    pub module_id: ModuleId,
    pub proto: Rc<FunctionProto>,
}

impl VmFactory {
    /// A fresh VM with the entry module registered, ready to run [`Self::closure`].
    pub fn build(&self) -> Vm {
        let mut machine = Vm::from_snapshot(
            self.globals.clone(),
            self.heap.clone(),
            self.precompiled.clone(),
            self.modules.clone(),
            varn_vm::ExecSettings::from_env(false),
        )
        .with_loader(self.loader.clone());

        let mut export_map = FxHashMap::default();
        for (idx, name) in self.proto.export_names.iter().enumerate() {
            export_map.insert(name.clone(), idx);
        }
        let mut module_obj =
            varn_types::ModuleObj::new(self.module_id.clone(), self.proto.export_names.len());
        module_obj.export_map = export_map;
        let module_val = machine.ctx.heap.alloc_module(Rc::new(module_obj));
        machine
            .ctx
            .modules
            .insert(self.module_id.clone(), module_val);
        machine.ctx.module_exports.insert(0, module_val);

        machine
    }

    pub fn closure(&self) -> Rc<Closure> {
        Rc::new(Closure::new(self.proto.clone(), Vec::new(), Vec::new()))
    }

    /// Build, run to completion, and hand the machine back for inspection.
    pub fn run_once(&self) -> Result<Vm, String> {
        let mut machine = self.build();
        run_vm_to_completion(&mut machine, self.closure())?;
        Ok(machine)
    }
}

pub fn run_vm_to_completion(machine: &mut Vm, closure: Rc<Closure>) -> Result<(), String> {
    loop {
        let res = machine.run(closure.clone());
        match res {
            Ok(_) => match machine.ctx.vm_suspend.take() {
                None => break,
                Some(varn_vm::exec::VmSuspend::Await { value, dest_reg }) => {
                    let res_val = match value {
                        varn_types::Value::Task(lazy) => {
                            let handle = machine.ctx.run_lazy_task_sync(lazy.as_ref());
                            match handle.peek_state() {
                                varn_types::TaskState::Resolved(v) => Ok(v),
                                varn_types::TaskState::Rejected(e) => Err(e),
                                _ => Ok(varn_types::Value::Null),
                            }
                        }
                        varn_types::Value::TaskHandle(handle) => match handle.peek_state() {
                            varn_types::TaskState::Resolved(v) => Ok(v),
                            varn_types::TaskState::Rejected(e) => Err(e),
                            _ => varn_vm::exec::ExecCtx::wait_task_handle_value(handle.clone()),
                        },
                        other => Ok(other),
                    };

                    match res_val {
                        Ok(resolved) => {
                            let resolved =
                                varn_vm::exec::host_values::open_resolved(&mut machine.ctx, resolved);
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
                                return Err(msg);
                            }
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
                return Err(msg);
            }
        }
    }
    Ok(())
}

/// Run `f` once untimed to warm caches, then `runs` timed iterations.
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

/// Like [`time_n`], but samples CPU frequency immediately after each run (CPU
/// still warm from the just-finished work) and keeps the peak.
#[allow(dead_code)]
pub fn time_n_freq<F: Fn() -> Result<(), String>>(
    runs: usize,
    f: F,
) -> Result<(Vec<Duration>, Option<crate::cpu_freq::CpuFreq>), CliError> {
    time_n_freq_setup(runs, || (), |_| f())
}

/// [`time_n_freq`] with a per-run SETUP step that is deliberately left out of
/// the measurement.
///
/// The execute phase builds a fresh VM per run, and that build deep-clones the
/// heap — whose cost scales with `NURSERY_CAPACITY`. Timing it made the harness
/// structurally biased against exactly the change an allocation-heavy workload
/// needs: growing the nursery from 16K to 256K slots measured 1.34x FASTER on
/// `bench_gc_alloc`'s own internal clock while this phase reported it 3x
/// slower, because each timed run was copying ~12 MB more before doing any
/// work. A benchmark that punishes a real improvement is worse than no
/// benchmark.
///
/// The setup still runs once per iteration, so each run starts from the same
/// fresh state it always did; only the clock moved.
pub fn time_n_freq_setup<T, S, F>(
    runs: usize,
    setup: S,
    f: F,
) -> Result<(Vec<Duration>, Option<crate::cpu_freq::CpuFreq>), CliError>
where
    S: Fn() -> T,
    F: Fn(&mut T) -> Result<(), String>,
{
    let mut warm = setup();
    f(&mut warm).map_err(|e| CliError::fatal(format!("bench warmup failed: {e}")))?;

    let mut samples = Vec::with_capacity(runs);
    let mut peak = None;
    for _ in 0..runs {
        let mut state = setup();
        let start = Instant::now();
        f(&mut state).map_err(|e| CliError::fatal(format!("bench run failed: {e}")))?;
        samples.push(start.elapsed());
        peak = crate::cpu_freq::keep_peak(peak, crate::cpu_freq::sample());
    }
    Ok((samples, peak))
}
