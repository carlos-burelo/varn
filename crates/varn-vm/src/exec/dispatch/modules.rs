use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::exec::dispatch::ControlSignal;
use crate::frame::VmClosure;

impl ExecCtx {
    #[inline(always)]
    pub(super) fn op_load_module(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        closure: &VmClosure,
        frame_idx: usize,
        dest_reg: usize,
    ) -> VmResult<ControlSignal> {
        let spec_idx = code[*ip] as usize;
        *ip += 1;
        let spec_nv = closure.constants[spec_idx];
        let spec = self
            .heap
            .str_val(spec_nv)
            .expect("OpLoadModule: not a string constant");

        self.frames[frame_idx].ip = *ip;
        let module_nv = self.load_module(&spec)?;
        if self.vm_suspend.is_some() {
            *ip -= 2;
            self.frames[frame_idx].ip = *ip;
            return Ok(ControlSignal::ContinueInstruction);
        }
        let base = self.frames[frame_idx].base;
        self.stack[base + dest_reg] = module_nv;
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_load_module_slot(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
        dest_reg: usize,
    ) -> VmResult<ControlSignal> {
        let w = code[*ip];
        *ip += 1;
        let src_reg = (w >> 8) as usize;
        let slot_idx = code[*ip] as usize;
        *ip += 1;

        let base = self.frames[frame_idx].base;
        let module_val = self.stack[base + src_reg];
        if !module_val.is_heap() {
            return Err(crate::error::RuntimeError::new(format!(
                "OpLoadModuleSlot: target is not a module object: {:?}",
                module_val
            )));
        }

        if let Some(crate::heap::HeapObj::Module(m)) = self.heap.get(module_val.as_heap_idx()) {
            if let Some(val) = m.get_slot(slot_idx) {
                self.stack[base + dest_reg] = val;
            } else {
                return Err(crate::error::RuntimeError::new(format!(
                    "OpLoadModuleSlot: slot {} out of bounds for module {}",
                    slot_idx,
                    m.id.as_str()
                )));
            }
        } else {
            return Err(crate::error::RuntimeError::new(format!(
                "OpLoadModuleSlot: target is not a ModuleObj heap object: {:?}",
                module_val
            )));
        }

        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_store_module_slot(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
        val_reg: usize,
    ) -> VmResult<ControlSignal> {
        let slot_idx = code[*ip] as usize;
        *ip += 1;

        let base = self.frames[frame_idx].base;
        let val_nv = self.stack[base + val_reg];

        let exports_nv = if let Some(nv) = self.module_exports.get(&frame_idx).copied() {
            nv
        } else {
            return Err(crate::error::RuntimeError::new(
                "OpStoreModuleSlot: no active module object for current frame".to_string(),
            ));
        };

        if !exports_nv.is_heap() {
            return Err(crate::error::RuntimeError::new(
                "OpStoreModuleSlot: active module object is not a heap object".to_string(),
            ));
        }

        if let Some(crate::heap::HeapObj::Module(m)) = self.heap.get_mut(exports_nv.as_heap_idx()) {
            let m = std::rc::Rc::make_mut(m);
            m.set_slot(slot_idx, val_nv);
        } else {
            return Err(crate::error::RuntimeError::new(
                "OpStoreModuleSlot: active module object is not a ModuleObj".to_string(),
            ));
        }

        Ok(ControlSignal::ContinueInstruction)
    }
}
