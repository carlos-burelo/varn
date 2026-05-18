use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::exec::dispatch::ControlSignal;
use crate::value::VmValue;

impl ExecCtx {
    #[inline(always)]
    pub(super) fn op_try(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
    ) -> ControlSignal {
        let w1 = code[*ip];
        *ip += 1;
        let err_reg = (w1 >> 8) as u8;
        let catch_ip_off = code[*ip] as usize;
        *ip += 1;
        let catch_ip = *ip + catch_ip_off;

        crate::exec::exceptions::push_try(
            &mut self.try_handlers,
            catch_ip,
            self.frames.len(),
            err_reg,
        );
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_pop_try(&mut self) -> ControlSignal {
        crate::exec::exceptions::pop_try(&mut self.try_handlers);
        ControlSignal::ContinueInstruction
    }
}
