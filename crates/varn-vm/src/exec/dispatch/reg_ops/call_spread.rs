use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

impl ExecCtx {
    pub(crate) fn exec_call_spread_reg(
        &mut self,
        callee: VmValue,
        base: usize,
        arg_start: usize,
        arg_count: usize,
        dest: usize,
        frame_idx: usize,
    ) -> VmResult<bool> {
        let mut expanded = Vec::new();
        let window: Vec<VmValue> = (0..arg_count)
            .map(|i| self.stack.box_reg(base, arg_start + i))
            .collect();
        crate::exec::calls::expand_spread_args(&self.heap, window, &mut expanded);
        let flat_count = expanded.len();
        self.stage.clear();
        self.stage.push(callee);
        for nv in expanded {
            self.stage.push(nv);
        }
        let prepared = self.prepare_call(callee, flat_count)?;
        self.dispatch_prepared_call(prepared)?;

        if self.frames.len() > frame_idx + 1 {
            self.frames.last_mut().unwrap().return_reg = dest as u16;
            return Ok(true);
        }

        let result = self.stage_pop();
        self.stack.unbox_into_reg(base, dest, result)?;
        Ok(false)
    }
}
