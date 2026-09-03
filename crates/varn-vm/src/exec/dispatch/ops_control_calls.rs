use crate::closure::VmClosure;
use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_core::OpCode;

use super::{hi, lo};

pub(super) enum ControlCallFlow {
    ContinueInstruction,
    ContinueFrame,
    Return(VmValue),
}

impl ExecCtx {
    /// Count one back edge and, when this proto crosses the OSR threshold,
    /// park a request for the frame loop. Returns `true` when a request was
    /// raised, which the caller turns into a `ContinueFrame` so the dispatch
    /// loop unwinds to the place that can actually enter compiled code.
    ///
    /// `header_ip` is the post-jump ip — the loop header, which the lowering's
    /// scan guarantees is a CLIF block start.
    #[inline(always)]
    fn note_backedge(&mut self, header_ip: usize, frame_idx: usize, closure: &VmClosure) -> bool {
        if self.settings.no_jit {
            return false;
        }
        let proto = &closure.proto;
        let n = proto.backedge_count.get().wrapping_add(1);
        if n < VmClosure::osr_backedge_threshold() {
            proto.backedge_count.set(n);
            return false;
        }
        // Crossed: raise a request and START OVER, rather than latching.
        //
        // The counter lives on the PROTO and outlives the frame, so a latch
        // would make OSR a once-per-process event: the second frame to enter
        // this function would be past the threshold before it began, raise
        // nothing, and interpret its whole loop while a perfectly good
        // compiled entry sat in `jit_osr_entry` unreachable. Resetting makes
        // every later frame re-request after another 1000 back edges, which
        // costs one cached lookup — `osr_jit_fn` compiles only once.
        //
        // It also bounds the cost of a refusal: a frame the guards keep
        // turning away pays one frame-loop round trip per 1000 back edges,
        // not one per back edge.
        proto.backedge_count.set(0);
        self.request_osr(header_ip, frame_idx, closure)
    }

    /// The cold half of [`Self::note_backedge`]: everything that only runs on
    /// the single back edge that crosses the threshold.
    #[cold]
    #[inline(never)]
    fn request_osr(&mut self, header_ip: usize, frame_idx: usize, closure: &VmClosure) -> bool {
        if closure.proto.jit_osr_failed.get() {
            return false;
        }
        // Compiled code does not know about the interpreter's handler stack,
        // and a handler pushed by THIS frame is live interpreter state the
        // compiled body would neither pop nor unwind through correctly. A
        // handler belonging to a frame below us is fine — we never reach it.
        if self
            .try_handlers
            .last()
            .is_some_and(|h| h.frame_depth >= self.frames.len())
        {
            return false;
        }
        // Only the top frame is executing; the dispatch loop guarantees it,
        // and the resume reads `frames[frame_idx].ip` back on the way round.
        debug_assert_eq!(frame_idx, self.frames.len() - 1);
        self.frames[frame_idx].ip = header_ip;
        self.osr_request = Some(header_ip);
        true
    }

    #[inline(always)]
    pub(super) fn exec_control_calls_op(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        frame_idx: usize,
        closure: &VmClosure,
        first_reg: usize,
        depth: usize,
    ) -> VmResult<Option<ControlCallFlow>> {
        match op {
            OpCode::Jump => {
                let offset = ((code[*ip] as u32) << 16 | code[*ip + 1] as u32) as usize;
                *ip += 2;
                *ip += offset;
                Ok(Some(ControlCallFlow::ContinueInstruction))
            }
            OpCode::Loop => {
                let offset = ((code[*ip] as u32) << 16 | code[*ip + 1] as u32) as usize;
                *ip += 2;
                *ip -= offset;
                // Collect on loop back-edges, not only at call boundaries. A long,
                // call-free allocating loop (e.g. string concatenation) would
                // otherwise never reach a GC check and exhaust memory.
                //
                // Unconditional, and deliberately NOT folded into the OSR test
                // below: a call-free allocating loop depends on this running on
                // EVERY back edge, whatever tiering decides.
                self.gc_backedge_safepoint();
                if self.note_backedge(*ip, frame_idx, closure) {
                    return Ok(Some(ControlCallFlow::ContinueFrame));
                }
                Ok(Some(ControlCallFlow::ContinueInstruction))
            }
            OpCode::JumpIfFalse => {
                let offset = ((code[*ip] as u32) << 16 | code[*ip + 1] as u32) as usize;
                *ip += 2;
                let cond = self.stack[base + first_reg];
                if !cond.is_truthy() {
                    *ip += offset;
                }
                Ok(Some(ControlCallFlow::ContinueInstruction))
            }
            OpCode::JumpIfTrue => {
                let offset = ((code[*ip] as u32) << 16 | code[*ip + 1] as u32) as usize;
                *ip += 2;
                let cond = self.stack[base + first_reg];
                if cond.is_truthy() {
                    *ip += offset;
                }
                Ok(Some(ControlCallFlow::ContinueInstruction))
            }
            OpCode::Return => {
                let w1 = code[*ip];
                let src = lo(w1);
                let res = self.reg_return(base, src);
                if self.frames.len() == depth {
                    Ok(Some(ControlCallFlow::Return(res)))
                } else {
                    Ok(Some(ControlCallFlow::ContinueFrame))
                }
            }
            OpCode::Call => {
                let w1 = code[*ip];
                *ip += 1;
                let w2 = code[*ip];
                *ip += 1;
                let (dest, callee_reg) = (hi(w1), lo(w1));
                let (arg_count, arg_start) = (hi(w2), lo(w2));
                self.frames[frame_idx].ip = *ip;
                let callee = self.stack[base + callee_reg];
                let jumped =
                    self.exec_call_reg(callee, base, arg_start, arg_count, dest, frame_idx)?;
                if jumped {
                    return Ok(Some(ControlCallFlow::ContinueFrame));
                }
                let frame_idx2 = self.frames.len() - 1;
                *ip = self.frames[frame_idx2].ip;
                Ok(Some(ControlCallFlow::ContinueInstruction))
            }
            OpCode::CallSelf => {
                let w1 = code[*ip];
                *ip += 1;
                let w2 = code[*ip];
                *ip += 1;
                let (dest, _) = (hi(w1), lo(w1));
                let (arg_count, arg_start) = (hi(w2), lo(w2));
                self.frames[frame_idx].ip = *ip;
                let jumped = self.exec_call_self(base, arg_start, arg_count, dest, frame_idx)?;
                if jumped {
                    return Ok(Some(ControlCallFlow::ContinueFrame));
                }
                let frame_idx2 = self.frames.len() - 1;
                *ip = self.frames[frame_idx2].ip;
                Ok(Some(ControlCallFlow::ContinueInstruction))
            }
            OpCode::CallMethod => {
                let cs = first_reg;
                let w1 = code[*ip];
                *ip += 1;
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let w3 = code[*ip];
                *ip += 1;
                let (dest, obj_reg) = (hi(w1), lo(w1));
                let (arg_count, arg_start) = (hi(w3), lo(w3));
                self.frames[frame_idx].ip = *ip;
                let this_val = self.stack[base + obj_reg];
                let jumped = self.exec_call_method_reg(
                    this_val, base, name_idx, cs, arg_start, arg_count, dest, frame_idx, closure,
                )?;
                if jumped {
                    return Ok(Some(ControlCallFlow::ContinueFrame));
                }
                let frame_idx2 = self.frames.len() - 1;
                *ip = self.frames[frame_idx2].ip;
                Ok(Some(ControlCallFlow::ContinueInstruction))
            }
            OpCode::InvokeVirtual => {
                let w1 = code[*ip];
                *ip += 1;
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let w3 = code[*ip];
                *ip += 1;
                let (dest, this_reg) = (hi(w1), lo(w1));
                let (arg_count, arg_start) = (hi(w3), lo(w3));
                self.frames[frame_idx].ip = *ip;
                let this_val = self.stack[base + this_reg];
                let jumped = self.exec_call_method_reg(
                    this_val,
                    base,
                    name_idx,
                    usize::MAX,
                    arg_start,
                    arg_count,
                    dest,
                    frame_idx,
                    closure,
                )?;
                if jumped {
                    return Ok(Some(ControlCallFlow::ContinueFrame));
                }
                let frame_idx2 = self.frames.len() - 1;
                *ip = self.frames[frame_idx2].ip;
                Ok(Some(ControlCallFlow::ContinueInstruction))
            }
            OpCode::CallSpread => {
                let w1 = code[*ip];
                *ip += 1;
                let w2 = code[*ip];
                *ip += 1;
                let (dest, callee_reg) = (hi(w1), lo(w1));
                let (arg_count, arg_start) = (hi(w2), lo(w2));
                self.frames[frame_idx].ip = *ip;
                let callee = self.stack[base + callee_reg];
                let jumped =
                    self.exec_call_spread_reg(callee, base, arg_start, arg_count, dest, frame_idx)?;
                if jumped {
                    return Ok(Some(ControlCallFlow::ContinueFrame));
                }
                let frame_idx2 = self.frames.len() - 1;
                *ip = self.frames[frame_idx2].ip;
                Ok(Some(ControlCallFlow::ContinueInstruction))
            }
            _ => Ok(None),
        }
    }
}
