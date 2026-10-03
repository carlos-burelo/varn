use super::*;

impl ExecCtx {
    pub(super) fn host_task_resolved(&mut self, value: VmValue) -> VmValue {
        let cell = crate::task::TaskCell::pending();
        crate::task::settle(&mut self.heap, &cell, Ok(value));
        crate::task::alloc_handle(&mut self.heap, cell)
    }
    pub(super) fn host_task_rejected(&mut self, value: VmValue) -> VmValue {
        let cell = crate::task::TaskCell::pending();
        crate::task::settle(&mut self.heap, &cell, Err(value));
        crate::task::alloc_handle(&mut self.heap, cell)
    }
    pub(super) fn host_spawn_isolate(
        &mut self,
        module_path: &str,
        export_name: &str,
        args: Vec<varn_types::value::SendValue>,
    ) -> Result<varn_types::HostPromise, String> {
        isolates::spawn_isolate(self, module_path, export_name, args)
    }
}
