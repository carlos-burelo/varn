use std::cell::RefCell;
use std::rc::Rc;

use crate::closure::VmClosure;
use crate::error::{RuntimeError, VmResult};
use crate::exec::scheduler::{freeze_frame, thaw_frame, FrameBuffers, FrozenFrame};
use crate::exec::{ExecCtx, VmSuspend};
use crate::frame::CallFrame;
use crate::value::VmValue;

enum Phase {
    Fresh {
        closure: Rc<VmClosure>,
        args: Vec<VmValue>,
        current_class: Option<Rc<varn_types::ClassObj>>,
    },
    Suspended {
        frame: FrozenFrame,
        dest_reg: u8,
    },
    Running,
    Done,
}

pub struct GenState {
    phase: RefCell<Phase>,
    spare: RefCell<FrameBuffers>,
    pub(crate) is_async: bool,
}

impl std::fmt::Debug for GenState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Generator(async={})", self.is_async)
    }
}

impl GenState {
    pub(crate) fn new(
        closure: Rc<VmClosure>,
        args: Vec<VmValue>,
        current_class: Option<Rc<varn_types::ClassObj>>,
    ) -> Rc<Self> {
        let is_async = closure.proto.is_async;
        Rc::new(GenState {
            phase: RefCell::new(Phase::Fresh {
                closure,
                args,
                current_class,
            }),
            spare: RefCell::new(FrameBuffers::default()),
            is_async,
        })
    }

    pub(crate) fn close(&self) {
        *self.phase.borrow_mut() = Phase::Done;
    }

    pub(crate) fn trace(&self, value: &mut dyn FnMut(VmValue)) {
        let Ok(phase) = self.phase.try_borrow() else {
            return;
        };
        match &*phase {
            Phase::Fresh { closure, args, .. } => {
                args.iter().for_each(|&v| value(v));
                for uv in &closure.upvalues {
                    value(uv.inner.borrow().value);
                }
            }
            Phase::Suspended { frame, .. } => frame.trace(value),
            Phase::Running | Phase::Done => {}
        }
    }
}

pub(crate) enum Entry {
    Done(VmValue),
    Mounted(usize),
}

impl ExecCtx {
    pub(crate) fn resume_generator(
        &mut self,
        generator: &Rc<GenState>,
        input: VmValue,
    ) -> VmResult<VmValue> {
        match self.enter_generator(generator, input)? {
            Entry::Done(v) => Ok(v),
            Entry::Mounted(depth) => self.run_generator(generator, depth),
        }
    }

    pub(crate) fn enter_generator(
        &mut self,
        generator: &Rc<GenState>,
        input: VmValue,
    ) -> VmResult<Entry> {
        let phase = std::mem::replace(&mut *generator.phase.borrow_mut(), Phase::Running);
        let depth = self.frames.len();
        match phase {
            Phase::Done => {
                *generator.phase.borrow_mut() = Phase::Done;
                Ok(Entry::Done(self.make_option(None)?))
            }
            Phase::Running => Err(RuntimeError::new("generator is already running")),
            Phase::Fresh {
                closure,
                args,
                current_class,
            } => {
                if self.call_depth() >= crate::frame::MAX_CALL_DEPTH {
                    *generator.phase.borrow_mut() = Phase::Done;
                    return Err(crate::frame::call_depth_exceeded());
                }
                let alloc = self.stack.push_frame(&closure.proto);
                let nregs = closure.proto.register_count as usize;
                self.stack.adopt_values(alloc, 0, &args, nregs);
                let mut frame = CallFrame::new_owned(closure, alloc);
                frame.current_class = current_class;
                self.frames.push(frame);
                Ok(Entry::Mounted(depth))
            }
            Phase::Suspended { frame, dest_reg } => {
                *generator.spare.borrow_mut() = thaw_frame(self, frame);
                let base = self.frames[depth].base;
                let _ = self.stack.unbox_into_reg(base, dest_reg as usize, input);
                Ok(Entry::Mounted(depth))
            }
        }
    }

    pub(crate) fn run_generator(
        &mut self,
        generator: &Rc<GenState>,
        depth: usize,
    ) -> VmResult<VmValue> {
        crate::exec::scheduler::request_osr(self, varn_types::ssa::OsrMode::Resume);
        loop {
            let outer = std::mem::replace(&mut self.jit_suspend_buf, std::ptr::null_mut());
            let ran = self.run_until(depth);
            self.jit_suspend_buf = outer;
            if let Err(e) = ran {
                self.abandon_generator(generator, depth);
                return Err(e);
            }
            match self.vm_suspend.take() {
                Some(VmSuspend::Yield { value, dest_reg }) => {
                    return self.park_generator(generator, depth, value, dest_reg);
                }
                Some(VmSuspend::Await { .. }) => {
                    self.abandon_generator(generator, depth);
                    return Err(RuntimeError::new(
                        "cannot use `await` inside a sync generator — declare it `async function*`",
                    ));
                }
                Some(VmSuspend::DebugBreak) => {}
                None => {
                    *generator.phase.borrow_mut() = Phase::Done;
                    return self.make_option(None);
                }
            }
        }
    }

    pub(crate) fn step_generator_task(&mut self, generator: &Rc<GenState>) -> VmValue {
        let output = crate::task::TaskCell::pending();
        crate::exec::scheduler::queue_for(self).push_ready(crate::exec::scheduler::ReadyTask {
            start: crate::exec::scheduler::Start::Step(Rc::clone(generator)),
            output: Rc::clone(&output),
            step: None,
        });
        crate::task::alloc_handle(&mut self.heap, output)
    }

    pub(crate) fn finish_step(
        &mut self,
        generator: &Rc<GenState>,
        suspend: Option<VmSuspend>,
    ) -> VmResult<VmValue> {
        match suspend {
            Some(VmSuspend::Yield { value, dest_reg }) => {
                self.park_generator(generator, 0, value, dest_reg)
            }
            Some(VmSuspend::Await { .. }) | Some(VmSuspend::DebugBreak) | None => {
                generator.close();
                self.make_option(None)
            }
        }
    }

    pub(crate) fn generator_entry(&mut self, depth: usize) -> Option<usize> {
        let frame = &self.frames[depth];
        let closure = frame.closure();
        if self.settings.no_jit {
            return None;
        }
        let entry = if frame.ip == 0 {
            closure.hot_jit_fn()
        } else {
            closure.osr_jit_fn(varn_types::ssa::OsrRequest {
                ip: frame.ip,
                mode: varn_types::ssa::OsrMode::Resume,
            })
        }?;
        let owner = depth + 1;
        self.try_handlers.retain(|h| h.frame_depth != owner);
        Some(entry as usize)
    }

    pub(crate) fn leave_generator(
        &mut self,
        generator: &Rc<GenState>,
        depth: usize,
    ) -> VmResult<VmValue> {
        match self.vm_suspend.take() {
            Some(VmSuspend::Yield { value, dest_reg }) => {
                self.park_generator(generator, depth, value, dest_reg)
            }
            Some(VmSuspend::Await { .. }) | Some(VmSuspend::DebugBreak) | None => {
                self.abandon_generator(generator, depth);
                self.make_option(None)
            }
        }
    }

    fn park_generator(
        &mut self,
        generator: &GenState,
        depth: usize,
        value: VmValue,
        dest_reg: u8,
    ) -> VmResult<VmValue> {
        let buffers = std::mem::take(&mut *generator.spare.borrow_mut());
        let frame = freeze_frame(self, depth, false, buffers).map_err(RuntimeError::new)?;
        self.frames.pop();
        self.stack.pop_frame();
        *generator.phase.borrow_mut() = Phase::Suspended { frame, dest_reg };
        self.make_option(Some(value))
    }

    pub(crate) fn abandon_generator(&mut self, generator: &GenState, depth: usize) {
        while self.frames.len() > depth {
            let frame = self.frames.pop().expect("a frame above the generator");
            self.drop_frame_storage(frame.base);
        }
        self.try_handlers.retain(|h| h.frame_depth <= depth);
        *generator.phase.borrow_mut() = Phase::Done;
    }
}
