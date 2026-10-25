use std::rc::Rc;
use std::time::{Duration, Instant};

use rustc_hash::FxHashMap;
use varn_compiler::FunctionProto;
use varn_core::ModuleId;
use varn_vm::Vm;

use crate::error::CliError;

pub struct VmFactory<'a> {
    session: &'a varn_pipeline::Session,
    precompiled: Rc<FxHashMap<ModuleId, Rc<FunctionProto>>>,
    proto: Rc<FunctionProto>,
    print_silent: bool,
    capabilities: varn_pipeline::CapabilitySet,
}

impl<'a> VmFactory<'a> {
    pub fn new(
        session: &'a varn_pipeline::Session,
        precompiled: Rc<FxHashMap<ModuleId, Rc<FunctionProto>>>,
        proto: Rc<FunctionProto>,
        print_silent: bool,
        capabilities: varn_pipeline::CapabilitySet,
    ) -> Result<Self, CliError> {
        let factory = Self {
            session,
            precompiled,
            proto,
            print_silent,
            capabilities,
        };
        factory.try_build()?;
        Ok(factory)
    }

    pub fn build(&self) -> Vm {
        match self.try_build() {
            Ok(machine) => machine,
            Err(error) => {
                unreachable!("initialization already succeeded in VmFactory::new: {error}")
            }
        }
    }

    pub fn build_silent(&self) -> Vm {
        let mut machine = self.build();
        machine.ctx.print_silent = true;
        machine
    }

    fn try_build(&self) -> Result<Vm, CliError> {
        let mut machine = varn_pipeline::boot_machine(
            self.session,
            Rc::clone(&self.precompiled),
            self.capabilities.clone(),
            false,
        )?;
        machine.ctx.print_silent = self.print_silent;
        varn_pipeline::enter_main(&mut machine, Rc::clone(&self.proto));
        Ok(machine)
    }

    pub fn entry_proto(&self) -> Rc<FunctionProto> {
        self.proto.clone()
    }

    pub fn run_once_silent(&self) -> Result<Vm, String> {
        let mut machine = self.build_silent();
        run_vm_to_completion(&mut machine, self.entry_proto())?;
        Ok(machine)
    }
}

pub fn run_vm_to_completion(machine: &mut Vm, entry: Rc<FunctionProto>) -> Result<(), String> {
    varn_pipeline::drive_entry(machine, &entry).map_err(|failure| failure.to_string())
}

fn compiled_keys() -> std::collections::BTreeSet<(String, usize)> {
    varn_jit::stats::take_records()
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

pub fn time_n_freq_setup<T, S, F, P>(
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
    varn_jit::stats::start_recording();
    let mut warm = setup();
    f(&mut warm).map_err(|e| CliError::fatal(format!("bench warmup failed: {e}")))?;
    let warmed = compiled_keys();
    varn_jit::stats::start_recording();

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
