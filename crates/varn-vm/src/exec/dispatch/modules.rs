use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::exec::dispatch::ControlSignal;
use crate::frame::VmClosure;

impl ExecCtx {
    #[inline(always)]
    pub(super) fn op_import(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        closure: &VmClosure,
        frame_idx: usize,
        first_reg: usize,
    ) -> VmResult<ControlSignal> {
        let spec_idx = code[*ip] as usize;
        *ip += 1;
        let spec_nv = closure.constants[spec_idx];
        let spec = self
            .heap
            .str_val(spec_nv)
            .expect("OpImport: not a string constant");

        self.frames[frame_idx].ip = *ip;
        let module_nv = self.load_module(&spec)?;
        if self.vm_suspend.is_some() {
            *ip -= 2;
            self.frames[frame_idx].ip = *ip;
            return Ok(ControlSignal::ContinueInstruction);
        }
        let base = self.frames[frame_idx].base;
        self.stack[base + first_reg] = module_nv;
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_merge_exports(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
        first_reg: usize,
    ) -> VmResult<ControlSignal> {
        let val_reg = first_reg;
        let key_idx = code[*ip] as usize;
        *ip += 1;
        let key = self.read_str_const_at(key_idx, frame_idx)?;
        let base = self.frames[frame_idx].base;
        let val_nv = self.stack[base + val_reg];

        let exports_nv = if let Some(nv) = self.module_exports.get(&frame_idx).copied() {
            nv
        } else {
            let nv = self.heap.intern(varn_types::Value::plain_object());
            self.module_exports.insert(frame_idx, nv);
            nv
        };

        crate::exec::modules::reexport(exports_nv, key.as_ref(), val_nv, &mut self.heap)?;
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_reexport(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<ControlSignal> {
        let idx = code[*ip] as usize;
        *ip += 1;
        let spec_nv = closure.constants[idx];
        let spec = self
            .heap
            .str_val(spec_nv)
            .expect("OpReexport: not a string constant");

        self.frames[frame_idx].ip = *ip;
        let module_id = crate::exec::modules::resolve_specifier_from_path(
            &spec,
            &closure.proto.chunk.source_file,
        )?;
        let resolved = module_id.as_str();

        let src_nv = if let Some(&cached) = self.modules.get(&resolved) {
            cached
        } else if let Some(nv) = varn_builtins::build_module(&resolved, &mut self.heap) {
            self.modules.insert(resolved.to_string(), nv);
            nv
        } else if let Some(proto) = self.precompiled.get(&resolved).cloned() {
            self.load_module(&resolved)?
        } else {
            return Err(crate::error::RuntimeError::new(format!(
                "module not found: {}",
                spec
            )));
        };

        let exports_nv = if let Some(nv) = self.module_exports.get(&frame_idx).copied() {
            nv
        } else {
            let nv = self.heap.intern(varn_types::Value::plain_object());
            self.module_exports.insert(frame_idx, nv);
            nv
        };

        crate::exec::modules::merge_exports(exports_nv, src_nv, &mut self.heap)?;
        Ok(ControlSignal::ContinueInstruction)
    }
}
