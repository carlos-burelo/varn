use super::*;

impl ExecCtx {
    pub(super) fn host_call_vm(
        &mut self,
        callee: VmValue,
        args: &[VmValue],
    ) -> Result<VmValue, varn_types::NativeError> {
        // The window is `[callee, args...]`, the exact shape the interpreter's
        // callee slot + arguments and the compiled caller's flushed staging
        // produce; `invoke` is the single run-to-completion entry.
        let mut window = Vec::with_capacity(args.len() + 1);
        window.push(callee);
        window.extend_from_slice(args);
        Ok(self.invoke(callee, &window)?)
    }
    pub(super) fn host_task_from_host(
        &mut self,
        promise: varn_types::HostPromise,
        open: varn_types::HostOpen,
    ) -> VmValue {
        let cell = crate::task::TaskCell::host(promise.clone(), open);
        let handle = crate::task::alloc_handle(&mut self.heap, std::rc::Rc::clone(&cell));
        crate::exec::scheduler::adopt_host(cell, &promise);
        handle
    }
    pub(super) fn host_task_cancel(&mut self, task: VmValue) -> Result<(), String> {
        let cell = match self.task_cell(task) {
            Some(cell) => cell,
            None => return Err("cancel: expected a task handle".to_string()),
        };
        if cell.is_yield() {
            return Ok(());
        }
        if let Some(promise) = cell.host_promise() {
            promise.reject_msg("Task cancelled");
        }
        let reason = self.heap.alloc_str("Task cancelled");
        crate::task::settle(&mut self.heap, &cell, Err(reason));
        Ok(())
    }
}
