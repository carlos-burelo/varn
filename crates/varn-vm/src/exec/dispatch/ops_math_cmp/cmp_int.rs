use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_core::OpCode;

use super::super::{hi, lo};

impl ExecCtx {
    pub(super) fn exec_int_cmp(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        first_reg: usize,
    ) -> VmResult<()> {
        let w1 = code[*ip];
        *ip += 1;
        let (r1, r2) = (hi(w1), lo(w1));
        match op {
            OpCode::LtInt => {
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_bool(a_val < b_val),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    let r = VmValue::from_bool(if a.is_int() && b.is_int() {
                        a.as_int() < b.as_int()
                    } else {
                        crate::exec::compare::lt_heap(a, b, &self.heap)
                    });
                    self.stack.unbox_into_reg(base, first_reg, r)?;
                }
            }
            OpCode::GtInt => {
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_bool(a_val > b_val),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    let r = VmValue::from_bool(if a.is_int() && b.is_int() {
                        a.as_int() > b.as_int()
                    } else {
                        crate::exec::compare::gt_heap(a, b, &self.heap)
                    });
                    self.stack.unbox_into_reg(base, first_reg, r)?;
                }
            }
            OpCode::LteInt => {
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_bool(a_val <= b_val),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    let r = VmValue::from_bool(if a.is_int() && b.is_int() {
                        a.as_int() <= b.as_int()
                    } else {
                        crate::exec::compare::lte_heap(a, b, &self.heap)
                    });
                    self.stack.unbox_into_reg(base, first_reg, r)?;
                }
            }
            OpCode::GteInt => {
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_bool(a_val >= b_val),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    let r = VmValue::from_bool(if a.is_int() && b.is_int() {
                        a.as_int() >= b.as_int()
                    } else {
                        crate::exec::compare::gte_heap(a, b, &self.heap)
                    });
                    self.stack.unbox_into_reg(base, first_reg, r)?;
                }
            }
            OpCode::EqInt => {
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_bool(a_val == b_val),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    let r = VmValue::from_bool(if a.is_int() && b.is_int() {
                        a.as_int() == b.as_int()
                    } else {
                        crate::exec::compare::eq(a, b, &self.heap)
                    });
                    self.stack.unbox_into_reg(base, first_reg, r)?;
                }
            }
            OpCode::NeqInt => {
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_bool(a_val != b_val),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    let r = VmValue::from_bool(if a.is_int() && b.is_int() {
                        a.as_int() != b.as_int()
                    } else {
                        crate::exec::compare::neq(a, b, &self.heap)
                    });
                    self.stack.unbox_into_reg(base, first_reg, r)?;
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }
}
