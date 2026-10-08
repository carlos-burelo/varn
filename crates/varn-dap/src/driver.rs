use std::collections::BTreeMap;
use std::rc::Rc;

use varn_pipeline::{
    boot_machine, compile_source_for_build, enter_main, CapabilitySet, DebugFlags,
};
use varn_types::FunctionProto;
use varn_vm::debug::{self, BreakAction, StepKind, StepState};
use varn_vm::Vm;

pub struct Driver {
    machine: Option<Vm>,
    main: Option<Rc<FunctionProto>>,
    protos: Vec<Rc<FunctionProto>>,
    wanted: BTreeMap<String, Vec<u32>>,
    program: String,
    finished: bool,
}

impl Driver {
    pub fn new() -> Self {
        Self {
            machine: None,
            main: None,
            protos: Vec::new(),
            wanted: BTreeMap::new(),
            program: String::new(),
            finished: false,
        }
    }

    pub fn launch(&mut self, program: &str, stop_on_entry: bool) -> Result<bool, String> {
        if self.machine.is_some() {
            return Err("already launched".to_string());
        }
        let canonical = std::path::Path::new(program)
            .canonicalize()
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(|e| format!("cannot resolve '{program}': {e}"))?;
        let source = std::fs::read_to_string(&canonical)
            .map_err(|e| format!("cannot read '{canonical}': {e}"))?;
        let session = varn_pipeline::resolver::Session::new();
        let compiled =
            compile_source_for_build(&source, &canonical, false, &DebugFlags::default(), &session)
                .map_err(|e| format!("compile failed: {e}"))?;
        let mut machine = boot_machine(compiled.precompiled, CapabilitySet::allow_all(), false)
            .map_err(|e| format!("boot failed: {e}"))?;
        machine.ctx.settings.no_jit = true;
        let main = enter_main(&mut machine, compiled.entry_proto);
        self.protos = collect_protos(&main, &machine.ctx.precompiled);
        self.program = canonical;
        self.main = Some(main);
        self.machine = Some(machine);
        self.refresh_table();
        if stop_on_entry {
            if let Some(m) = self.machine.as_mut() {
                m.ctx.debug_step = Some(StepState {
                    kind: StepKind::In,
                    depth: usize::MAX,
                    line: u32::MAX,
                });
            }
        }
        Ok(stop_on_entry)
    }

    pub fn set_breakpoints(&mut self, path: &str, lines: &[u32]) -> Vec<bool> {
        let canonical = std::path::Path::new(path)
            .canonicalize()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_owned());
        if lines.is_empty() {
            self.wanted.remove(&canonical);
        } else {
            self.wanted.insert(canonical.clone(), lines.to_vec());
        }
        self.refresh_table();
        lines
            .iter()
            .map(|l| self.line_resolved(&canonical, *l))
            .collect()
    }

    fn line_resolved(&self, path: &str, line: u32) -> bool {
        self.protos.iter().any(|p| {
            p.chunk.source_file.as_ref() == path && !debug::pcs_for_line(p, line).is_empty()
        })
    }

    fn refresh_table(&mut self) {
        let mut entries = Vec::new();
        for (path, lines) in &self.wanted {
            for proto in &self.protos {
                if proto.chunk.source_file.as_ref() != path {
                    continue;
                }
                let ptr = Rc::as_ptr(proto) as usize;
                for line in lines {
                    for pc in debug::pcs_for_line(proto, *line) {
                        entries.push((ptr, pc));
                    }
                }
            }
        }
        let table = debug::build_break_table(&entries);
        if let Some(m) = self.machine.as_mut() {
            m.ctx.debug_breaks = Some(table);
        }
    }

    pub fn resume(&mut self) -> ResumeOutcome {
        if self.machine.is_none() {
            return ResumeOutcome::Error("not launched".to_string());
        }
        if self.finished {
            return ResumeOutcome::Terminated;
        }
        if let Some(m) = self.machine.as_mut() {
            arm_skip(m);
        }
        self.drive_once()
    }

    pub fn step(&mut self, kind: StepKind) -> ResumeOutcome {
        let (depth, line) = match self.machine.as_ref() {
            Some(m) => (m.ctx.frames.len(), current_line(m)),
            None => return ResumeOutcome::Error("not launched".to_string()),
        };
        if self.finished {
            return ResumeOutcome::Terminated;
        }
        if let Some(m) = self.machine.as_mut() {
            arm_skip(m);
            m.ctx.debug_step = Some(StepState { kind, depth, line });
        }
        self.drive_once()
    }

    fn drive_once(&mut self) -> ResumeOutcome {
        let main = match self.main.clone() {
            Some(m) => m,
            None => return ResumeOutcome::Error("not launched".to_string()),
        };
        let machine = match self.machine.as_mut() {
            Some(m) => m,
            None => return ResumeOutcome::Error("not launched".to_string()),
        };
        let mut on_break = |_: &mut Vm| BreakAction::Halt;
        match debug::drive_main(machine, &main, &mut on_break) {
            debug::DriveResult::Done(_) => {
                self.finished = true;
                ResumeOutcome::Terminated
            }
            debug::DriveResult::Stopped => ResumeOutcome::Stopped,
            debug::DriveResult::Failed(e) => {
                self.finished = true;
                ResumeOutcome::RuntimeError(format_runtime_error(&e.message, &e.frames))
            }
        }
    }

    pub fn machine(&self) -> Option<&Vm> {
        self.machine.as_ref()
    }

    pub fn is_launched(&self) -> bool {
        self.machine.is_some()
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn disconnect(&mut self) {
        self.machine = None;
        self.main = None;
        self.protos.clear();
        self.finished = true;
    }
}

impl Default for Driver {
    fn default() -> Self {
        Self::new()
    }
}

fn arm_skip(machine: &mut Vm) {
    let depth = machine.ctx.frames.len();
    if depth == 0 {
        return;
    }
    let frame = &machine.ctx.frames[depth - 1];
    let ptr = std::rc::Rc::as_ptr(unsafe { &(*frame.closure_ptr).proto }) as usize;
    machine.ctx.debug_skip = Some((ptr, frame.ip));
}

fn current_line(machine: &Vm) -> u32 {
    let depth = machine.ctx.frames.len();
    if depth == 0 {
        return 0;
    }
    let frame = &machine.ctx.frames[depth - 1];
    let proto = unsafe { &(*frame.closure_ptr).proto }.clone();
    proto.chunk.lines.get_line(frame.ip)
}

fn collect_protos(
    main: &Rc<FunctionProto>,
    precompiled: &Rc<rustc_hash::FxHashMap<varn_core::ModuleId, Rc<FunctionProto>>>,
) -> Vec<Rc<FunctionProto>> {
    let mut out = Vec::new();
    let mut seen = rustc_hash::FxHashSet::default();
    let mut stack: Vec<Rc<FunctionProto>> = vec![main.clone()];
    stack.extend(precompiled.values().cloned());
    while let Some(proto) = stack.pop() {
        let ptr = Rc::as_ptr(&proto) as usize;
        if !seen.insert(ptr) {
            continue;
        }
        for entry in &proto.chunk.constants {
            if let varn_types::PoolEntry::Function(child) = entry {
                stack.push(child.clone());
            }
        }
        out.push(proto);
    }
    out
}

fn format_runtime_error(message: &str, frames: &[varn_vm::FrameInfo]) -> String {
    let mut msg = message.to_owned();
    for frame in frames {
        msg.push_str(&format!(
            "\n    at {} ({}:{})",
            frame.fn_name, frame.file, frame.line
        ));
    }
    msg
}

pub enum ResumeOutcome {
    Stopped,
    Terminated,
    RuntimeError(String),
    Error(String),
}
