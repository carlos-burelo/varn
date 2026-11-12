use super::ctx::ExecCtx;
use crate::value::VmValue;

impl ExecCtx {
    pub(crate) fn for_each_jit_slot(&self, mut visit: impl FnMut(*mut VmValue)) {
        unsafe {
            vn_jit::stack_roots::for_each_slot(self.jit_exit, &mut visit);
            for exit in self.jit_exits_saved.iter().rev() {
                vn_jit::stack_roots::for_each_slot(*exit, &mut visit);
            }
        }
    }
}
