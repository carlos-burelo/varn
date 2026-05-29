use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::frame::VmClosure;
use varn_core::OpCode;

use super::{hi, lo};

impl ExecCtx {
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
                self.stack[base + first_reg] = crate::value::VmValue::null();
            }
            OpCode::LoadTrue => {
                self.stack[base + first_reg] = crate::value::VmValue::bool_true();
            }
            OpCode::LoadFalse => {
                self.stack[base + first_reg] = crate::value::VmValue::bool_false();
            }
            OpCode::LoadInt => {
                let val = code[*ip] as i16;
                *ip += 1;
                self.stack[base + first_reg] = crate::value::VmValue::from_int(val as i64);
            }
            OpCode::LoadIntZero => {
                self.stack[base + first_reg] = crate::value::VmValue::from_int(0);
            }
            OpCode::LoadIntOne => {
                self.stack[base + first_reg] = crate::value::VmValue::from_int(1);
            }
            OpCode::LoadIntMinusOne => {
                self.stack[base + first_reg] = crate::value::VmValue::from_int(-1);
            }
            OpCode::LoadConst => {
                let cidx = code[*ip] as usize;
                *ip += 1;
                let nv = closure.constants[cidx];
                self.stack[base + first_reg] = nv;
            }
            OpCode::Move => {
                let w1 = code[*ip];
                *ip += 1;
                self.stack[base + first_reg] = self.stack[base + hi(w1)];
            }
            OpCode::LoadGlobalIdx => {
                let gidx = code[*ip] as usize;
                *ip += 1;
                debug_assert!(
                    gidx < self.globals.values.len(),
                    "LoadGlobalIdx out of bounds: {gidx} >= {}",
                    self.globals.values.len()
                );
                self.stack[base + first_reg] = self.globals.values[gidx];
            }
            OpCode::StoreGlobalIdx | OpCode::DefineGlobalIdx => {
                let src = (code[*ip] >> 8) as usize;
                let gidx = code[*ip + 1] as usize;
                *ip += 2;
                debug_assert!(
                    gidx < self.globals.values.len(),
                    "StoreGlobalIdx out of bounds: {gidx} >= {}",
                    self.globals.values.len()
                );
                let val = self.stack[base + src];
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
                self.stack[base + dest] = closure.upvalues[uv].read(&self.stack);
            }
            OpCode::StoreUpvalue => {
                let w1 = code[*ip];
                *ip += 1;
                let (uv, src) = (hi(w1), lo(w1));
                let val = self.stack[base + src];
                closure.upvalues[uv].write(val, &mut self.stack);
            }
            OpCode::CloseUpvalue => {
                let w1 = code[*ip];
                *ip += 1;
                let lowest = hi(w1);
                self.close_upvalues_above(base + lowest);
            }
            _ => return Ok(false),
        }

        Ok(true)
    }
}
