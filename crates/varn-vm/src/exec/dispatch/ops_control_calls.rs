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
    
    
    
    
    
    
    
    #[inline(always)]
    fn note_backedge(&mut self, header_ip: usize, frame_idx: usize, closure: &VmClosure) -> bool {
        if self.settings.no_jit {
            return false;
        }
        if crate::jit::tiering::FRAME_LAYOUT_V2_JIT_BAIL {
            return false;
        }
        let proto = &closure.proto;
        let n = proto.backedge_count.get().wrapping_add(1);
        if n < VmClosure::osr_backedge_threshold() {
            proto.backedge_count.set(n);
            return false;
        }
        
        
        
        
        
        
        
        
        
        
        
        
        
        proto.backedge_count.set(0);
        self.request_osr(header_ip, frame_idx, closure)
    }

    
    
    #[cold]
    #[inline(never)]
    fn request_osr(&mut self, header_ip: usize, frame_idx: usize, closure: &VmClosure) -> bool {
        if closure.proto.jit_osr_failed.get() {
            return false;
        }
        
        
        
        
        if self
            .try_handlers
            .last()
            .is_some_and(|h| h.frame_depth >= self.frames.len())
        {
            return false;
        }
        
        
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
                
                
                
                
                
                
                
                self.gc_backedge_safepoint();
                if self.note_backedge(*ip, frame_idx, closure) {
                    return Ok(Some(ControlCallFlow::ContinueFrame));
                }
                Ok(Some(ControlCallFlow::ContinueInstruction))
            }
            OpCode::JumpIfFalse => {
                let offset = ((code[*ip] as u32) << 16 | code[*ip + 1] as u32) as usize;
                *ip += 2;
                let cond = self.stack.box_reg(base, first_reg);
                if !cond.is_truthy() {
                    *ip += offset;
                }
                Ok(Some(ControlCallFlow::ContinueInstruction))
            }
            OpCode::JumpIfTrue => {
                let offset = ((code[*ip] as u32) << 16 | code[*ip + 1] as u32) as usize;
                *ip += 2;
                let cond = self.stack.box_reg(base, first_reg);
                if cond.is_truthy() {
                    *ip += offset;
                }
                Ok(Some(ControlCallFlow::ContinueInstruction))
            }
            OpCode::Return => {
                let w1 = code[*ip];
                let src = lo(w1);
                let res = self.reg_return(base, src)?;
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
                let callee = self.stack.box_reg(base, callee_reg);
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
                let this_val = self.stack.box_reg(base, obj_reg);
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
                let this_val = self.stack.box_reg(base, this_reg);
                let jumped = self.exec_call_method_reg(
                    this_val, base, name_idx, first_reg, arg_start, arg_count, dest, frame_idx,
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
                let callee = self.stack.box_reg(base, callee_reg);
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
