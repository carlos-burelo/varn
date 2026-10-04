use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

impl ExecCtx {
    pub(super) fn reg_return(&mut self, base: usize, src: usize) -> VmResult<VmValue> {
        let val = self.stack.box_reg(base, src);
        let returning_frame_idx = self.frames.len().saturating_sub(1);
        let frame = self.frames.pop().unwrap();
        self.drop_frame_storage(frame.base);

        let is_module_frame = frame.closure().proto.name.as_deref() == Some("<module>")
            && !frame.closure().proto.chunk.source_file.is_empty();

        let final_val =
            crate::exec::frame_ctrl::resolve_constructor_return(self, returning_frame_idx, val);

        if is_module_frame {
            let source_file = frame.closure().proto.chunk.source_file.to_string();
            let module_exports = self.module_exports.remove(&returning_frame_idx);
            let cached = module_exports.unwrap_or(final_val);
            let module_id = varn_core::ModuleId::from_canonical_str(&source_file);
            unsafe { &mut *self.modules.get() }.insert(module_id, cached);
        }

        if frame.return_reg != crate::frame::CallFrame::NO_RETURN_REG {
            // Sin llamante (retorno del frame raíz) no hay destino: equivale
            // a NO_RETURN_REG. Con llamante, conversión a su clase.
            if let Some(caller) = self.frames.last() {
                let caller_base = caller.base;
                self.stack
                    .unbox_into_reg(caller_base, frame.return_reg as usize, final_val)?;
            }
        }
        Ok(final_val)
    }

    pub(crate) fn exec_typeof(&self, v: VmValue) -> &'static str {
        use varn_core::RuntimeKind;
        if v.is_null() {
            return RuntimeKind::Null.name();
        }
        if v.is_int() {
            return RuntimeKind::Int.name();
        }
        if v.is_f64() {
            return RuntimeKind::Float.name();
        }
        if v.is_bool() {
            return RuntimeKind::Bool.name();
        }
        if v.is_sso() {
            return RuntimeKind::Str.name();
        }
        if !v.is_heap() {
            return "unknown";
        }
        match self.heap.get(v.as_heap()) {
            Some(obj) => obj.tag().name(),
            None => "unknown",
        }
    }
}
