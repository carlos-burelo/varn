use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::exec::dispatch::ControlSignal;
use crate::value::VmValue;

impl ExecCtx {
    #[inline(always)]
    pub(super) fn op_jump(&mut self, code: &[u16], ip: &mut usize) -> ControlSignal {
        let offset = code[*ip] as usize;
        *ip += 1;
        *ip += offset;
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_jump_if_false(&mut self, code: &[u16], ip: &mut usize) -> ControlSignal {
        let offset = code[*ip] as usize;
        *ip += 1;
        let cond = unsafe {
            let new_len = self.stack.len() - 1;
            let v = self.stack[new_len];
            self.stack.set_len(new_len);
            v
        };
        if !cond.is_truthy() {
            *ip += offset;
        }
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_jump_if_true(&mut self, code: &[u16], ip: &mut usize) -> ControlSignal {
        let offset = code[*ip] as usize;
        *ip += 1;
        let cond = unsafe {
            let new_len = self.stack.len() - 1;
            let v = self.stack[new_len];
            self.stack.set_len(new_len);
            v
        };
        if cond.is_truthy() {
            *ip += offset;
        }
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_loop(&mut self, code: &[u16], ip: &mut usize) -> ControlSignal {
        let offset = code[*ip] as usize;
        *ip += 1;
        *ip -= offset;
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(crate) fn op_return(&mut self, depth: usize) -> VmResult<ControlSignal> {
        let result = unsafe {
            if self.stack.is_empty() {
                VmValue::null()
            } else {
                let new_len = self.stack.len() - 1;
                let v = self.stack[new_len];
                self.stack.set_len(new_len);
                v
            }
        };
        let res = self.do_return(result)?;
        if self.frames.len() == depth {
            return Ok(ControlSignal::Return(res));
        }
        Ok(ControlSignal::ContinueFrame)
    }
}
