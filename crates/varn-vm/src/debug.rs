use rustc_hash::FxHashMap;
use std::rc::Rc;

use crate::error::RuntimeError;
use crate::exec::VmSuspend;
use crate::value::VmValue;
use crate::Vm;

pub type BreakTable = FxHashMap<(usize, usize), ()>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    In,
    Over,
    Out,
}

#[derive(Clone, Copy)]
pub struct StepState {
    pub kind: StepKind,
    pub depth: usize,
    pub line: u32,
}

impl StepState {
    pub fn should_stop(&self, depth: usize, line: u32) -> bool {
        match self.kind {
            StepKind::In => line != self.line,
            StepKind::Over => depth <= self.depth && line != self.line,
            StepKind::Out => depth < self.depth,
        }
    }
}

pub(crate) fn check_break(
    ctx: *mut crate::exec::ExecCtx,
    closure_ptr: *const crate::closure::VmClosure,
    frame_idx: usize,
    ip: usize,
) -> bool {
    unsafe {
        let c = &*ctx;
        let closure = &*closure_ptr;
        let proto_ptr = Rc::as_ptr(&closure.proto) as usize;
        let here = (proto_ptr, ip);
        if (*ctx).debug_skip == Some(here) {
            (*ctx).debug_skip = None;
            return false;
        }
        if let Some(table) = c.debug_breaks.as_ref() {
            if table.contains_key(&(proto_ptr, ip)) {
                let m = &mut *ctx;
                m.frames[frame_idx].ip = ip;
                m.vm_suspend = Some(VmSuspend::DebugBreak);
                return true;
            }
        }
        if let Some(step) = c.debug_step.as_ref() {
            let step = *step;
            let line = closure.proto.chunk.lines.get_line(ip);
            let depth = c.frames.len();
            if step.should_stop(depth, line) {
                let m = &mut *ctx;
                m.frames[frame_idx].ip = ip;
                m.vm_suspend = Some(VmSuspend::DebugBreak);
                m.debug_step = None;
                return true;
            }
        }
    }
    false
}

pub fn build_break_table(entries: &[(usize, usize)]) -> Rc<BreakTable> {
    let mut table = BreakTable::default();
    for e in entries {
        table.insert(*e, ());
    }
    Rc::new(table)
}

pub fn pcs_for_line(proto: &varn_types::FunctionProto, line: u32) -> Vec<usize> {
    let mapping = &proto.chunk.lines;
    let mut out = Vec::new();
    let mut base = 0usize;
    for entry in &mapping.entries {
        if entry.line == line {
            out.push(base);
        }
        base += entry.count as usize;
    }
    out
}

#[derive(Clone, Debug)]
pub struct DebugFrame {
    pub fn_name: String,
    pub file: String,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub struct DebugVar {
    pub name: String,
    pub value: String,
    pub reg: usize,
}

pub fn snapshot_frames(ctx: &crate::exec::ExecCtx) -> Vec<DebugFrame> {
    let depth = ctx.frames.len();
    let mut out = Vec::with_capacity(depth);
    for (i, f) in ctx.frames.iter().rev().enumerate() {
        let proto = unsafe { &*f.closure_ptr }.proto.clone();
        let fn_name = proto
            .name
            .as_deref()
            .map(|s| s.to_owned())
            .unwrap_or_else(|| "<anonymous>".to_owned());
        let raw_file = proto.chunk.source_file.as_ref();
        let file = match raw_file.strip_prefix(r"\\?\") {
            Some(stripped) => stripped.to_owned(),
            None => raw_file.to_owned(),
        };
        let raw_line = if i == 0 {
            proto.chunk.lines.get_line(f.ip)
        } else {
            proto.chunk.lines.get_line(f.ip.saturating_sub(1))
        };
        let line = if raw_line > 0 { raw_line } else { 1 };
        out.push(DebugFrame {
            fn_name,
            file,
            line,
        });
    }
    out
}

pub fn snapshot_vars(ctx: &crate::exec::ExecCtx, frame_index: usize) -> Vec<DebugVar> {
    let depth = ctx.frames.len();
    if frame_index >= depth {
        return Vec::new();
    }
    let frame = &ctx.frames[depth - 1 - frame_index];
    let proto = unsafe { &*frame.closure_ptr }.proto.clone();
    let nregs = proto.register_count as usize;
    let mut out = Vec::with_capacity(nregs);
    for r in 0..nregs {
        let val: VmValue = ctx.stack.box_reg(frame.base, r);
        out.push(DebugVar {
            name: format!("r{r}"),
            value: ctx.heap.str_repr(val),
            reg: r,
        });
    }
    out
}

pub enum BreakAction {
    Resume,
    Halt,
}

pub enum DriveResult {
    Done(VmValue),
    Stopped,
    Failed(RuntimeError),
}

pub fn drive_main(
    machine: &mut Vm,
    main: &Rc<varn_types::FunctionProto>,
    on_break: &mut dyn FnMut(&mut Vm) -> BreakAction,
) -> DriveResult {
    loop {
        match machine.run(main.clone()) {
            Ok(v) => match machine.ctx.vm_suspend.take() {
                None => return DriveResult::Done(v),
                Some(VmSuspend::Await { value, dest_reg }) => {
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
                            let err = crate::exec::exceptions::build_thrown_error(
                                thrown,
                                &machine.ctx.heap,
                                &machine.ctx.frames,
                            );
                            if let Some(handler) = machine.ctx.try_handlers.pop() {
                                let thrown_val = err.thrown.unwrap_or(varn_types::VmValue::null());
                                let _ = crate::exec::frame_ctrl::unwind_to_handler(
                                    &mut machine.ctx,
                                    handler,
                                    thrown_val,
                                );
                            } else {
                                let mut err = err;
                                err.message = format!("awaited task failed: {}", err.message);
                                return DriveResult::Failed(err);
                            }
                        }
                    }
                }
                Some(VmSuspend::Yield { .. }) => {}
                Some(VmSuspend::DebugBreak) => match on_break(machine) {
                    BreakAction::Resume => {}
                    BreakAction::Halt => return DriveResult::Stopped,
                },
            },
            Err(e) => return DriveResult::Failed(e),
        }
    }
}
