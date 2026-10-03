use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_core::OpCode;

use super::super::{hi, lo};

#[inline(always)]
pub(super) fn float_fast(a: VmValue, b: VmValue, heap: &crate::heap::Heap) -> Option<(f64, f64)> {
    if a.is_f64() && b.is_f64() {
        Some((a.as_f64(), b.as_f64()))
    } else if (a.is_f64() || heap.is_int(a)) && (b.is_f64() || heap.is_int(b)) {
        Some((mixed_to_f64(a, heap), mixed_to_f64(b, heap)))
    } else {
        None
    }
}

#[inline(always)]
pub(super) fn mixed_to_f64(v: VmValue, heap: &crate::heap::Heap) -> f64 {
    if v.is_f64() {
        v.as_f64()
    } else {
        heap.to_f64_val(v)
    }
}

impl ExecCtx {
    pub(super) fn exec_float_arith(
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
            OpCode::AddFloat => {
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    self.stack
                        .unbox_into_reg(base, first_reg, VmValue::from_f64(a_val + b_val))?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    match float_fast(a, b, &self.heap) {
                        Some((av, bv)) => {
                            self.stack.unbox_into_reg(
                                base,
                                first_reg,
                                VmValue::from_f64(av + bv),
                            )?;
                        }
                        None => {
                            let r = crate::exec::arith::add(a, b, &mut self.heap)?;
                            self.stack.unbox_into_reg(base, first_reg, r)?;
                        }
                    };
                }
            }
            OpCode::SubFloat => {
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    self.stack
                        .unbox_into_reg(base, first_reg, VmValue::from_f64(a_val - b_val))?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    match float_fast(a, b, &self.heap) {
                        Some((av, bv)) => {
                            self.stack.unbox_into_reg(
                                base,
                                first_reg,
                                VmValue::from_f64(av - bv),
                            )?;
                        }
                        None => {
                            let r = crate::exec::arith::sub(a, b, &mut self.heap)?;
                            self.stack.unbox_into_reg(base, first_reg, r)?;
                        }
                    };
                }
            }
            OpCode::MulFloat => {
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    self.stack
                        .unbox_into_reg(base, first_reg, VmValue::from_f64(a_val * b_val))?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    match float_fast(a, b, &self.heap) {
                        Some((av, bv)) => {
                            self.stack.unbox_into_reg(
                                base,
                                first_reg,
                                VmValue::from_f64(av * bv),
                            )?;
                        }
                        None => {
                            let r = crate::exec::arith::mul(a, b, &mut self.heap)?;
                            self.stack.unbox_into_reg(base, first_reg, r)?;
                        }
                    };
                }
            }
            OpCode::DivFloat => {
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    self.stack
                        .unbox_into_reg(base, first_reg, VmValue::from_f64(a_val / b_val))?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    match float_fast(a, b, &self.heap) {
                        Some((av, bv)) => {
                            self.stack.unbox_into_reg(
                                base,
                                first_reg,
                                VmValue::from_f64(av / bv),
                            )?;
                        }
                        None => {
                            let r = crate::exec::arith::div(a, b, &mut self.heap)?;
                            self.stack.unbox_into_reg(base, first_reg, r)?;
                        }
                    };
                }
            }
            OpCode::ModFloat => {
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    self.stack
                        .unbox_into_reg(base, first_reg, VmValue::from_f64(a_val % b_val))?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    match float_fast(a, b, &self.heap) {
                        Some((av, bv)) => {
                            self.stack.unbox_into_reg(
                                base,
                                first_reg,
                                VmValue::from_f64(av % bv),
                            )?;
                        }
                        None => {
                            let r = crate::exec::arith::modulo(a, b, &mut self.heap)?;
                            self.stack.unbox_into_reg(base, first_reg, r)?;
                        }
                    };
                }
            }
            OpCode::PowFloat => {
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    self.stack.unbox_into_reg(
                        base,
                        first_reg,
                        VmValue::from_f64(a_val.powf(b_val)),
                    )?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    match float_fast(a, b, &self.heap) {
                        Some((av, bv)) => {
                            self.stack.unbox_into_reg(
                                base,
                                first_reg,
                                VmValue::from_f64(av.powf(bv)),
                            )?;
                        }
                        None => {
                            let r = crate::exec::arith::pow(a, b, &mut self.heap)?;
                            self.stack.unbox_into_reg(base, first_reg, r)?;
                        }
                    };
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }
}
