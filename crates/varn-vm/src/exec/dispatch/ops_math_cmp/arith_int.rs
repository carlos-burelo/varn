use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_core::OpCode;

use super::super::{hi, lo};
use super::int_overflow;

impl ExecCtx {
    pub(super) fn exec_imm(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        first_reg: usize,
    ) -> VmResult<()> {
        let w1 = code[*ip];
        *ip += 1;
        let src = hi(w1);
        let imm = lo(w1) as i8 as i64;
        match op {
            OpCode::AddImm => {
                if self.stack.reg_class(base, src) == crate::frame_store::SlotClass::Gpr {
                    let a_val = self.stack.g(base, src);
                    match varn_core::add_int(a_val, imm) {
                        Some(r) => {
                            self.stack
                                .unbox_into_reg(base, first_reg, VmValue::from_int(r))?
                        }
                        None => return Err(int_overflow("+", a_val, imm)),
                    }
                } else {
                    let v = self.stack.box_reg(base, src);
                    if self.heap.is_int(v) {
                        let a_val = self.heap.as_int(v);
                        match varn_core::add_int(a_val, imm) {
                            Some(r) => {
                                self.stack
                                    .unbox_into_reg(base, first_reg, VmValue::from_int(r))?
                            }
                            None => return Err(int_overflow("+", a_val, imm)),
                        };
                    } else {
                        let imm_v = VmValue::from_int(imm);
                        let r = crate::exec::arith::add(v, imm_v, &mut self.heap)?;
                        self.stack.unbox_into_reg(base, first_reg, r)?;
                    }
                }
            }
            OpCode::SubImm => {
                if self.stack.reg_class(base, src) == crate::frame_store::SlotClass::Gpr {
                    let a_val = self.stack.g(base, src);
                    match varn_core::sub_int(a_val, imm) {
                        Some(r) => {
                            self.stack
                                .unbox_into_reg(base, first_reg, VmValue::from_int(r))?
                        }
                        None => return Err(int_overflow("-", a_val, imm)),
                    }
                } else {
                    let v = self.stack.box_reg(base, src);
                    if self.heap.is_int(v) {
                        let a_val = self.heap.as_int(v);
                        match varn_core::sub_int(a_val, imm) {
                            Some(r) => {
                                self.stack
                                    .unbox_into_reg(base, first_reg, VmValue::from_int(r))?
                            }
                            None => return Err(int_overflow("-", a_val, imm)),
                        };
                    } else {
                        let imm_v = VmValue::from_int(imm);
                        let r = crate::exec::arith::sub(v, imm_v, &mut self.heap)?;
                        self.stack.unbox_into_reg(base, first_reg, r)?;
                    }
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    pub(super) fn exec_int_arith(
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
            OpCode::AddInt => {
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    match varn_core::add_int(a_val, b_val) {
                        Some(r) => {
                            self.stack
                                .unbox_into_reg(base, first_reg, VmValue::from_int(r))?
                        }
                        None => return Err(int_overflow("+", a_val, b_val)),
                    }
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    if a.is_int() && b.is_int() {
                        let a_val = a.as_int();
                        let b_val = b.as_int();
                        match varn_core::add_int(a_val, b_val) {
                            Some(r) => {
                                self.stack
                                    .unbox_into_reg(base, first_reg, VmValue::from_int(r))?
                            }
                            None => return Err(int_overflow("+", a_val, b_val)),
                        }
                    } else {
                        let r = crate::exec::arith::add(a, b, &mut self.heap)?;
                        self.stack.unbox_into_reg(base, first_reg, r)?;
                    };
                }
            }
            OpCode::SubInt => {
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    match varn_core::sub_int(a_val, b_val) {
                        Some(r) => {
                            self.stack
                                .unbox_into_reg(base, first_reg, VmValue::from_int(r))?
                        }
                        None => return Err(int_overflow("-", a_val, b_val)),
                    }
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    if a.is_int() && b.is_int() {
                        let a_val = a.as_int();
                        let b_val = b.as_int();
                        match varn_core::sub_int(a_val, b_val) {
                            Some(r) => {
                                self.stack
                                    .unbox_into_reg(base, first_reg, VmValue::from_int(r))?
                            }
                            None => return Err(int_overflow("-", a_val, b_val)),
                        }
                    } else {
                        let r = crate::exec::arith::sub(a, b, &mut self.heap)?;
                        self.stack.unbox_into_reg(base, first_reg, r)?;
                    };
                }
            }
            OpCode::MulInt => {
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    match varn_core::mul_int(a_val, b_val) {
                        Some(r) => {
                            self.stack
                                .unbox_into_reg(base, first_reg, VmValue::from_int(r))?
                        }
                        None => return Err(int_overflow("*", a_val, b_val)),
                    }
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    if a.is_int() && b.is_int() {
                        let a_val = a.as_int();
                        let b_val = b.as_int();
                        match varn_core::mul_int(a_val, b_val) {
                            Some(r) => {
                                self.stack
                                    .unbox_into_reg(base, first_reg, VmValue::from_int(r))?
                            }
                            None => return Err(int_overflow("*", a_val, b_val)),
                        }
                    } else {
                        let r = crate::exec::arith::mul(a, b, &mut self.heap)?;
                        self.stack.unbox_into_reg(base, first_reg, r)?;
                    };
                }
            }
            OpCode::DivInt => {
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    let r = varn_core::div_int(a_val, b_val)
                        .map_err(|f| crate::exec::arith::int_div_fault(f, "/", a_val, b_val))?;
                    self.stack
                        .unbox_into_reg(base, first_reg, VmValue::from_int(r))?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    if a.is_int() && b.is_int() {
                        let a_val = a.as_int();
                        let b_val = b.as_int();
                        let r = varn_core::div_int(a_val, b_val)
                            .map_err(|f| crate::exec::arith::int_div_fault(f, "/", a_val, b_val))?;
                        self.stack
                            .unbox_into_reg(base, first_reg, VmValue::from_int(r))?;
                    } else {
                        let r = crate::exec::arith::div(a, b, &mut self.heap)?;
                        self.stack.unbox_into_reg(base, first_reg, r)?;
                    };
                }
            }
            OpCode::ModInt => {
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    let r = varn_core::rem_int(a_val, b_val)
                        .map_err(|f| crate::exec::arith::int_div_fault(f, "%", a_val, b_val))?;
                    self.stack
                        .unbox_into_reg(base, first_reg, VmValue::from_int(r))?;
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    if a.is_int() && b.is_int() {
                        let a_val = a.as_int();
                        let b_val = b.as_int();
                        let r = varn_core::rem_int(a_val, b_val)
                            .map_err(|f| crate::exec::arith::int_div_fault(f, "%", a_val, b_val))?;
                        self.stack
                            .unbox_into_reg(base, first_reg, VmValue::from_int(r))?;
                    } else {
                        let r = crate::exec::arith::modulo(a, b, &mut self.heap)?;
                        self.stack.unbox_into_reg(base, first_reg, r)?;
                    };
                }
            }
            OpCode::PowInt => {
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    if b_val < 0 {
                        return Err(crate::error::RuntimeError::new(
                            "negative exponent in integer power",
                        ));
                    }
                    let e = u32::try_from(b_val).unwrap_or(u32::MAX);
                    match varn_core::pow_int(a_val, e) {
                        Some(r) => {
                            self.stack
                                .unbox_into_reg(base, first_reg, VmValue::from_int(r))?
                        }
                        None => return Err(int_overflow("**", a_val, b_val)),
                    }
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    if a.is_int() && b.is_int() {
                        let a_val = a.as_int();
                        let b_val = b.as_int();
                        if b_val < 0 {
                            return Err(crate::error::RuntimeError::new(
                                "negative exponent in integer power",
                            ));
                        }
                        let e = u32::try_from(b_val).unwrap_or(u32::MAX);
                        match varn_core::pow_int(a_val, e) {
                            Some(r) => {
                                self.stack
                                    .unbox_into_reg(base, first_reg, VmValue::from_int(r))?
                            }
                            None => return Err(int_overflow("**", a_val, b_val)),
                        }
                    } else {
                        let r = crate::exec::arith::pow(a, b, &mut self.heap)?;
                        self.stack.unbox_into_reg(base, first_reg, r)?;
                    };
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }
}
