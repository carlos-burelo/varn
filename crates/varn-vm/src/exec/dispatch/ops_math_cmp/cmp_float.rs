use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_core::OpCode;

use super::super::{hi, lo};

impl ExecCtx {
    pub(super) fn exec_float_cmp(
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
            OpCode::LtFloat => {
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_bool(a_val < b_val),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    let r = VmValue::from_bool(
                        if (a.is_f64() || self.heap.is_int(a))
                            && (b.is_f64() || self.heap.is_int(b))
                        {
                            self.heap.to_f64_val(a) < self.heap.to_f64_val(b)
                        } else {
                            crate::exec::compare::lt_heap(a, b, &self.heap)
                        },
                    );
                    self.stack.unbox_into_reg(base, first_reg, r)?;
                }
            }
            OpCode::GtFloat => {
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_bool(a_val > b_val),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    let r = VmValue::from_bool(
                        if (a.is_f64() || self.heap.is_int(a))
                            && (b.is_f64() || self.heap.is_int(b))
                        {
                            self.heap.to_f64_val(a) > self.heap.to_f64_val(b)
                        } else {
                            crate::exec::compare::gt_heap(a, b, &self.heap)
                        },
                    );
                    self.stack.unbox_into_reg(base, first_reg, r)?;
                }
            }
            OpCode::LteFloat => {
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_bool(a_val <= b_val),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    let r = VmValue::from_bool(
                        if (a.is_f64() || self.heap.is_int(a))
                            && (b.is_f64() || self.heap.is_int(b))
                        {
                            self.heap.to_f64_val(a) <= self.heap.to_f64_val(b)
                        } else {
                            crate::exec::compare::lte_heap(a, b, &self.heap)
                        },
                    );
                    self.stack.unbox_into_reg(base, first_reg, r)?;
                }
            }
            OpCode::GteFloat => {
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_bool(a_val >= b_val),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    let r = VmValue::from_bool(
                        if (a.is_f64() || self.heap.is_int(a))
                            && (b.is_f64() || self.heap.is_int(b))
                        {
                            self.heap.to_f64_val(a) >= self.heap.to_f64_val(b)
                        } else {
                            crate::exec::compare::gte_heap(a, b, &self.heap)
                        },
                    );
                    self.stack.unbox_into_reg(base, first_reg, r)?;
                }
            }
            OpCode::EqFloat => {
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_bool(a_val == b_val),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    let r = VmValue::from_bool(
                        if (a.is_f64() || self.heap.is_int(a))
                            && (b.is_f64() || self.heap.is_int(b))
                        {
                            self.heap.to_f64_val(a) == self.heap.to_f64_val(b)
                        } else {
                            crate::exec::compare::eq(a, b, &self.heap)
                        },
                    );
                    self.stack.unbox_into_reg(base, first_reg, r)?;
                }
            }
            OpCode::NeqFloat => {
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_bool(a_val != b_val),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    let r = VmValue::from_bool(
                        if (a.is_f64() || self.heap.is_int(a))
                            && (b.is_f64() || self.heap.is_int(b))
                        {
                            self.heap.to_f64_val(a) != self.heap.to_f64_val(b)
                        } else {
                            crate::exec::compare::neq(a, b, &self.heap)
                        },
                    );
                    self.stack.unbox_into_reg(base, first_reg, r)?;
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }
}
