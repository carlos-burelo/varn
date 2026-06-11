use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::frame::VmClosure;
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
                let method_name_nv = closure.constants[name_idx];
                let method_name = self
                    .heap
                    .str_val(method_name_nv)
                    .expect("InvokeVirtual: not a string const");
                let method_nv =
                    crate::exec::props::get_property(this_val, &method_name, &mut self.heap)?;
                let jumped =
                    self.exec_call_reg(method_nv, base, arg_start, arg_count, dest, frame_idx)?;
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
