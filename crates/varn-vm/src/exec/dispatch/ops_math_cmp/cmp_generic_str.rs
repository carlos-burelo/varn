use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_core::OpCode;

use super::super::{hi, lo};

impl ExecCtx {
    pub(super) fn exec_generic_cmp(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        first_reg: usize,
    ) -> VmResult<()> {
        let w1 = code[*ip];
        *ip += 1;
        let a = self.stack.box_reg(base, hi(w1));
        let b = self.stack.box_reg(base, lo(w1));
        let r = match op {
            OpCode::Eq => VmValue::from_bool(crate::exec::compare::eq(a, b, &self.heap)),
            OpCode::Neq => VmValue::from_bool(crate::exec::compare::neq(a, b, &self.heap)),
            OpCode::Lt => VmValue::from_bool(crate::exec::compare::lt_heap(a, b, &self.heap)),
            OpCode::Lte => VmValue::from_bool(crate::exec::compare::lte_heap(a, b, &self.heap)),
            OpCode::Gt => VmValue::from_bool(crate::exec::compare::gt_heap(a, b, &self.heap)),
            OpCode::Gte => VmValue::from_bool(crate::exec::compare::gte_heap(a, b, &self.heap)),
            _ => unreachable!(),
        };
        self.stack.unbox_into_reg(base, first_reg, r)?;
        Ok(())
    }

    pub(super) fn exec_str_op(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        first_reg: usize,
    ) -> VmResult<()> {
        match op {
            OpCode::ToString => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack.box_reg(base, src);
                let r = crate::exec::strings::to_string(v, &mut self.heap);
                self.stack.unbox_into_reg(base, first_reg, r)?;
            }
            OpCode::StrConcat => {
                let w1 = code[*ip];
                *ip += 1;
                let a = self.stack.box_reg(base, hi(w1));
                let b = self.stack.box_reg(base, lo(w1));
                let r = crate::exec::strings::str_concat(a, b, &mut self.heap);
                self.stack.unbox_into_reg(base, first_reg, r)?;
            }
            OpCode::BuildStr => {
                let count = hi(code[*ip]);
                *ip += 1;
                let mut out = crate::strbuf::StrBuf::new();
                for i in 0..count {
                    let reg_idx = hi(code[*ip + i]);
                    self.heap
                        .str_repr_into(self.stack.box_reg(base, reg_idx), &mut out);
                }
                *ip += count;
                let r = self.heap.alloc_str_dynamic(out.as_str());
                self.stack.unbox_into_reg(base, first_reg, r)?;
            }
            OpCode::StrLength => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack.box_reg(base, src);
                let r = self.exec_str_length(v)?;
                self.stack.unbox_into_reg(base, first_reg, r)?;
            }
            OpCode::StrSlice => {
                let w1 = code[*ip];
                *ip += 1;
                let s = self.stack.box_reg(base, hi(w1));
                let idx = self.stack.box_reg(base, lo(w1));
                let r = self.exec_str_slice(s, idx)?;
                self.stack.unbox_into_reg(base, first_reg, r)?;
            }
            _ => unreachable!(),
        }
        Ok(())
    }
}
