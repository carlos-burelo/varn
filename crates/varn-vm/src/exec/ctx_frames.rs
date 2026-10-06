use std::rc::Rc;

use crate::closure::{VmClosure, VmUpvalue};
use crate::error::VmResult;
use crate::frame_store::SlotAddr;
use crate::value::VmValue;

use super::calls::PreparedCall;
use super::ctx::ExecCtx;

impl ExecCtx {
    #[inline(always)]
    pub(crate) fn prepare_call(
        &mut self,
        callee_nv: VmValue,
        arg_count: usize,
    ) -> VmResult<PreparedCall> {
        if let Some((prepared, needs_receiver)) = super::calls::try_prepare_call_fast(
            callee_nv,
            arg_count,
            &self.stage,
            &self.heap,
            &mut self.stack,
        ) {
            if needs_receiver && callee_nv.is_heap() {
                let receiver_clone = if let Some(crate::heap::HeapObj::BoundMethod(bm)) =
                    self.heap.get(callee_nv.as_heap())
                {
                    Some(bm.receiver)
                } else {
                    None
                };
                if let Some(receiver) = receiver_clone {
                    let recv_nv = receiver;
                    match prepared {
                        PreparedCall::Frame(ref frame) => {
                            
                            
                            self.stack.unbox_into_reg(frame.base, 0, recv_nv)?;
                        }
                        PreparedCall::NativeImmediate(_, _)
                        | PreparedCall::RawNativeImmediate(_, _) => {
                            if self.stage.is_empty() {
                                self.stage.push(recv_nv);
                            } else {
                                self.stage[0] = recv_nv;
                            }
                        }
                        _ => {}
                    }
                }
            }
            return Ok(prepared);
        }

        self.record_call_slow();
        super::calls::prepare_call(
            callee_nv,
            arg_count,
            &mut self.stage,
            &mut self.heap,
            self.settings,
            &mut self.stack,
        )
    }

    pub(crate) fn push_frame(&mut self, closure: Rc<VmClosure>) -> crate::error::VmResult<()> {
        if self.frames.len() >= crate::frame::MAX_CALL_DEPTH {
            return Err(crate::error::RuntimeError::new(
                "stack overflow: call depth exceeded 10000",
            ));
        }
        let alloc = self.stack.push_frame(&closure.proto);
        self.frames
            .push(crate::frame::CallFrame::new_owned(closure, alloc));
        Ok(())
    }

    
    
    pub(crate) fn stage_pop(&mut self) -> VmValue {
        self.stage.pop().unwrap_or(VmValue::null())
    }

    
    
    
    
    
    
    
    
    
    
    pub(crate) fn push_call_frame(
        &mut self,
        proto: &Rc<varn_types::FunctionProto>,
        src_base: usize,
        src_start: usize,
        arg_count: usize,
    ) -> VmResult<usize> {
        let alloc = self.stack.push_frame(proto);
        for i in 0..arg_count {
            if let Err(e) = self.stack.mov_cross(alloc, i, src_base, src_start + i) {
                self.stack.pop_frame();
                return Err(e);
            }
        }
        Ok(alloc)
    }

    
    
    
    
    
    
    
    
    pub(crate) fn push_call_frame_with_this(
        &mut self,
        proto: &Rc<varn_types::FunctionProto>,
        this_val: VmValue,
        args: crate::exec::method_args::MethodArgs<'_>,
    ) -> VmResult<usize> {
        let alloc = self.stack.push_frame(proto);
        let filled = self
            .stack
            .unbox_into_reg(alloc, 0, this_val)
            .and_then(|()| args.copy_into(&mut self.stack, alloc, 1, args.len()));
        if let Err(e) = filled {
            self.stack.pop_frame();
            return Err(e);
        }
        Ok(alloc)
    }

    pub(crate) fn capture_upvalue(&mut self, slot: SlotAddr) -> VmUpvalue {
        for (s, uv) in &self.open_upvalues {
            if *s == slot {
                return uv.clone();
            }
        }
        let up = VmUpvalue::open(slot);
        self.open_upvalues.push((slot, up.clone()));
        self.open_upvalues.sort_by_key(|(s, _)| *s);
        up
    }

    
    
    
    
    pub(crate) fn drop_frame_storage(&mut self, alloc: usize) {
        if alloc == crate::frame::CallFrame::NO_ACTIVATION {
            return;
        }
        self.close_upvalues_in(alloc);
        self.stack.pop_frame();
    }

    pub(crate) fn close_upvalues_in(&mut self, alloc: usize) {
        if self.open_upvalues.is_empty() || alloc == crate::frame::CallFrame::NO_ACTIVATION {
            return;
        }
        let bases = self.stack.alloc_bases(alloc);
        for (s, uv) in self.open_upvalues.iter().rev() {
            if s.idx >= bases[s.class.index()] {
                uv.close(&self.stack);
            }
        }
        self.open_upvalues
            .retain(|(s, _)| s.idx < bases[s.class.index()]);
    }

    
    
    pub(crate) fn close_upvalues_from_reg(&mut self, alloc: usize, lowest: usize) {
        if self.open_upvalues.is_empty() {
            return;
        }
        let mut to_close = Vec::new();
        for (s, _) in self.open_upvalues.iter() {
            if let Some(reg) = self.stack.reg_of_addr(alloc, *s) {
                if reg >= lowest {
                    to_close.push(*s);
                }
            }
        }
        for s in &to_close {
            if let Some((_, uv)) = self.open_upvalues.iter().find(|(a, _)| a == s) {
                uv.close(&self.stack);
            }
        }
        self.open_upvalues.retain(|(s, _)| !to_close.contains(s));
    }
}
