use crate::closure::VmClosure;
use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use varn_core::OpCode;

use super::{hi, lo};

impl ExecCtx {
    #[inline(always)]
    pub(super) fn exec_literals_vars_op(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        frame_idx: usize,
        closure: &VmClosure,
        first_reg: usize,
    ) -> VmResult<bool> {
        match op {
            OpCode::LoadNull => {
                self.stack
                    .unbox_into_reg(base, first_reg, crate::value::VmValue::null())?;
            }
            OpCode::LoadTrue => {
                self.stack
                    .unbox_into_reg(base, first_reg, crate::value::VmValue::bool_true())?;
            }
            OpCode::LoadFalse => {
                self.stack
                    .unbox_into_reg(base, first_reg, crate::value::VmValue::bool_false())?;
            }
            OpCode::LoadInt => {
                let val = code[*ip] as i16;
                *ip += 1;
                self.stack.unbox_into_reg(
                    base,
                    first_reg,
                    crate::value::VmValue::from_int(val as i64),
                )?;
            }
            OpCode::LoadIntZero => {
                self.stack
                    .unbox_into_reg(base, first_reg, crate::value::VmValue::from_int(0))?;
            }
            OpCode::LoadIntOne => {
                self.stack
                    .unbox_into_reg(base, first_reg, crate::value::VmValue::from_int(1))?;
            }
            OpCode::LoadIntMinusOne => {
                self.stack
                    .unbox_into_reg(base, first_reg, crate::value::VmValue::from_int(-1))?;
            }
            OpCode::LoadConst => {
                let cidx = code[*ip] as usize;
                *ip += 1;
                let nv = closure.constants[cidx];
                self.stack.unbox_into_reg(base, first_reg, nv)?;
            }
            OpCode::Move => {
                let w1 = code[*ip];
                *ip += 1;
                // Los registros siempre están dentro de `register_count` (el
                // compilador los dimensiona); el `resize` anterior era defensa
                // muerta. `mov` convierte entre clases con chequeo.
                self.stack.mov(base, first_reg, hi(w1))?;
            }
            OpCode::LoadGlobalIdx => {
                let gidx = closure.module_base as usize + code[*ip] as usize;
                *ip += 1;
                debug_assert!(
                    gidx < self.globals.values.len(),
                    "LoadGlobalIdx out of bounds: {gidx} >= {}",
                    self.globals.values.len()
                );
                let nv = self.globals.values[gidx];
                self.stack.unbox_into_reg(base, first_reg, nv)?;
                self.record_hotspot_global(gidx);
            }
            OpCode::LoadNativeGlobalIdx => {
                let gidx = code[*ip] as usize;
                *ip += 1;
                debug_assert!(
                    gidx < self.globals.values.len(),
                    "LoadNativeGlobalIdx out of bounds: {gidx} >= {}",
                    self.globals.values.len()
                );
                let nv = self.globals.values[gidx];
                self.stack.unbox_into_reg(base, first_reg, nv)?;
                self.record_hotspot_global(gidx);
            }
            OpCode::StoreGlobalIdx | OpCode::DefineGlobalIdx => {
                let src = (code[*ip] >> 8) as usize;
                let gidx = closure.module_base as usize + code[*ip + 1] as usize;
                *ip += 2;
                debug_assert!(
                    gidx < self.globals.values.len(),
                    "StoreGlobalIdx out of bounds: {gidx} >= {}",
                    self.globals.values.len()
                );
                let val = self.stack.box_reg(base, src);
                self.globals.set_by_index_unchecked(gidx, val);
            }
            OpCode::LoadGlobal | OpCode::StoreGlobal | OpCode::DefineGlobal => {
                self.frames[frame_idx].ip = *ip;
                self.exec_variable_op(op, code, ip, base, frame_idx, closure, first_reg)?;
            }
            OpCode::LoadUpvalue => {
                let w1 = code[*ip];
                *ip += 1;
                let (dest, uv) = (hi(w1), lo(w1));
                let nv = closure.upvalues[uv].read(&self.stack);
                self.stack.unbox_into_reg(base, dest, nv)?;
            }
            OpCode::StoreUpvalue => {
                let w1 = code[*ip];
                *ip += 1;
                let (uv, src) = (hi(w1), lo(w1));
                let val = self.stack.box_reg(base, src);
                closure.upvalues[uv].write(val, &mut self.stack)?;
            }
            OpCode::CloseUpvalue => {
                let w1 = code[*ip];
                *ip += 1;
                let lowest = hi(w1);
                self.close_upvalues_from_reg(base, lowest);
            }
            _ => return Ok(false),
        }

        Ok(true)
    }
}
