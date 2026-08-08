use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::closure::VmClosure;
use crate::value::VmValue;
use varn_core::OpCode;

mod calls;
mod class_ops;
mod get_property;
mod method_calls;
mod misc_ops;
mod set_property;

impl ExecCtx {
    pub(super) fn exec_variable_op(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        frame_idx: usize,
        closure: &VmClosure,
        first_reg: usize,
    ) -> VmResult<()> {
        match op {
            OpCode::LoadGlobal => {
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let name_nv = closure.constants[name_idx];
                let name = self
                    .heap
                    .str_val(name_nv)
                    .ok_or_else(|| RuntimeError::new("LoadGlobal: non-string const"))?;
                let val = self.globals.get_by_name(&name).unwrap_or(VmValue::null());
                self.stack[base + first_reg] = val;
            }
            OpCode::StoreGlobal => {
                let src = (code[*ip] >> 8) as usize;
                *ip += 1;
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let name_nv = closure.constants[name_idx];
                let name = self
                    .heap
                    .str_val(name_nv)
                    .ok_or_else(|| RuntimeError::new("StoreGlobal: non-string const"))?;
                let val = self.stack[base + src];
                self.globals.set_by_name(&name, val);
            }
            OpCode::DefineGlobal => {
                let src = (code[*ip] >> 8) as usize;
                *ip += 1;
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let name_nv = closure.constants[name_idx];
                let name = self
                    .heap
                    .str_val(name_nv)
                    .ok_or_else(|| RuntimeError::new("DefineGlobal: non-string const"))?;
                let val = self.stack[base + src];
                self.globals.define(&name, val);
            }
            OpCode::LoadGlobalIdx => {
                let idx = code[*ip] as usize;
                *ip += 1;
                let val = self.globals.get_by_index(idx).unwrap_or(VmValue::null());
                self.stack[base + first_reg] = val;
                self.record_hotspot_global(idx);
            }
            OpCode::StoreGlobalIdx => {
                let src = (code[*ip] >> 8) as usize;
                *ip += 1;
                let idx = code[*ip] as usize;
                *ip += 1;
                let val = self.stack[base + src];
                self.globals.set_by_index(idx, val);
            }
            OpCode::DefineGlobalIdx => {
                let src = (code[*ip] >> 8) as usize;
                *ip += 1;
                let idx = code[*ip] as usize;
                *ip += 1;
                let val = self.stack[base + src];
                self.globals.set_by_index(idx, val);
            }
            _ => {}
        }
        self.frames[frame_idx].ip = *ip;
        Ok(())
    }
}
