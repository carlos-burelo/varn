use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::frame::VmClosure;
use crate::value::VmValue;
use varn_core::OpCode;

use super::{hi, lo};

impl ExecCtx {
    #[inline(always)]
    pub(super) fn exec_math_cmp_op(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        _frame_idx: usize,
        _closure: &VmClosure,
        first_reg: usize,
    ) -> VmResult<bool> {
        match op {
            OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Mod
            | OpCode::Pow
            | OpCode::BitAnd
            | OpCode::BitOr
            | OpCode::BitXor
            | OpCode::Shl
            | OpCode::Shr
            | OpCode::Ushr => {
                let w1 = code[*ip];
                *ip += 1;
                let (src1, src2) = (hi(w1), lo(w1));
                let a = self.stack[base + src1];
                let b = self.stack[base + src2];
                let r = self.exec_arith(op, a, b)?;
                self.stack[base + first_reg] = r;
            }
            OpCode::Negate => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack[base + src];
                self.stack[base + first_reg] = crate::exec::arith::negate(v, &mut self.heap);
            }
            OpCode::Not => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack[base + src];
                self.stack[base + first_reg] = crate::exec::compare::logical_not(v);
            }
            OpCode::AddImm => {
                let w1 = code[*ip];
                *ip += 1;
                let src = hi(w1);
                let imm = lo(w1) as i8 as i64;
                let v = self.stack[base + src];
                if v.is_int() {
                    let (r, overflow) = v.as_int().overflowing_add(imm);
                    self.stack[base + first_reg] = if overflow {
                        VmValue::from_f64(v.as_int() as f64 + imm as f64)
                    } else {
                        VmValue::from_int(r)
                    };
                } else {
                    self.stack[base + first_reg] =
                        crate::exec::arith::add(v, VmValue::from_int(imm), &mut self.heap)?;
                }
            }
            OpCode::SubImm => {
                let w1 = code[*ip];
                *ip += 1;
                let src = hi(w1);
                let imm = lo(w1) as i8 as i64;
                let v = self.stack[base + src];
                if v.is_int() {
                    let (r, overflow) = v.as_int().overflowing_sub(imm);
                    self.stack[base + first_reg] = if overflow {
                        VmValue::from_f64(v.as_int() as f64 - imm as f64)
                    } else {
                        VmValue::from_int(r)
                    };
                } else {
                    self.stack[base + first_reg] =
                        crate::exec::arith::sub(v, VmValue::from_int(imm), &mut self.heap)?;
                }
            }
            OpCode::AddInt
            | OpCode::SubInt
            | OpCode::MulInt
            | OpCode::DivInt
            | OpCode::ModInt
            | OpCode::PowInt => {
                let w1 = code[*ip];
                *ip += 1;
                let (src1, src2) = (hi(w1), lo(w1));
                let a = self.stack[base + src1];
                let b = self.stack[base + src2];
                let res = if a.is_int() && b.is_int() {
                    // Integer semantics (varn_core::numeric): arithmetic
                    // wraps at 48 bits — `from_int` masks the payload, so a
                    // plain wrapping op is exact and tier-identical with the
                    // JIT. `int / int` always yields float; `**` stays
                    // integral and rejects negative exponents.
                    match op {
                        OpCode::AddInt => VmValue::from_int(a.as_int().wrapping_add(b.as_int())),
                        OpCode::SubInt => VmValue::from_int(a.as_int().wrapping_sub(b.as_int())),
                        OpCode::MulInt => VmValue::from_int(a.as_int().wrapping_mul(b.as_int())),
                        OpCode::DivInt => {
                            let bv = b.as_int();
                            if bv == 0 {
                                return Err(crate::error::RuntimeError::new("division by zero"));
                            }
                            VmValue::from_f64(a.as_int() as f64 / bv as f64)
                        }
                        OpCode::ModInt => {
                            let bv = b.as_int();
                            if bv == 0 {
                                return Err(crate::error::RuntimeError::new("modulo by zero"));
                            }
                            VmValue::from_int(a.as_int() % bv)
                        }
                        OpCode::PowInt => {
                            let exp = b.as_int();
                            if exp < 0 {
                                return Err(crate::error::RuntimeError::new(
                                    "negative exponent in integer power",
                                ));
                            }
                            let e = u32::try_from(exp).unwrap_or(u32::MAX);
                            VmValue::from_int(a.as_int().wrapping_pow(e))
                        }
                        _ => unreachable!(),
                    }
                } else {
                    let generic_op = match op {
                        OpCode::AddInt => OpCode::Add,
                        OpCode::SubInt => OpCode::Sub,
                        OpCode::MulInt => OpCode::Mul,
                        OpCode::DivInt => OpCode::Div,
                        OpCode::ModInt => OpCode::Mod,
                        OpCode::PowInt => OpCode::Pow,
                        _ => unreachable!(),
                    };
                    self.exec_arith(generic_op, a, b)?
                };
                self.stack[base + first_reg] = res;
            }
            OpCode::LtInt
            | OpCode::GtInt
            | OpCode::LteInt
            | OpCode::GteInt
            | OpCode::EqInt
            | OpCode::NeqInt => {
                let w1 = code[*ip];
                *ip += 1;
                let (src1, src2) = (hi(w1), lo(w1));
                let a = self.stack[base + src1];
                let b = self.stack[base + src2];
                let res = if a.is_int() && b.is_int() {
                    let cmp_res = match op {
                        OpCode::LtInt => a.as_int() < b.as_int(),
                        OpCode::GtInt => a.as_int() > b.as_int(),
                        OpCode::LteInt => a.as_int() <= b.as_int(),
                        OpCode::GteInt => a.as_int() >= b.as_int(),
                        OpCode::EqInt => a.as_int() == b.as_int(),
                        OpCode::NeqInt => a.as_int() != b.as_int(),
                        _ => unreachable!(),
                    };
                    VmValue::from_bool(cmp_res)
                } else {
                    let generic_op = match op {
                        OpCode::LtInt => OpCode::Lt,
                        OpCode::GtInt => OpCode::Gt,
                        OpCode::LteInt => OpCode::Lte,
                        OpCode::GteInt => OpCode::Gte,
                        OpCode::EqInt => OpCode::Eq,
                        OpCode::NeqInt => OpCode::Neq,
                        _ => unreachable!(),
                    };
                    self.exec_cmp(generic_op, a, b)
                };
                self.stack[base + first_reg] = res;
            }
            OpCode::AddFloat
            | OpCode::SubFloat
            | OpCode::MulFloat
            | OpCode::DivFloat
            | OpCode::ModFloat
            | OpCode::PowFloat => {
                let w1 = code[*ip];
                *ip += 1;
                let (src1, src2) = (hi(w1), lo(w1));
                let a = self.stack[base + src1];
                let b = self.stack[base + src2];
                let res = if (a.is_f64() || a.is_int()) && (b.is_f64() || b.is_int()) {
                    match op {
                        OpCode::AddFloat => VmValue::from_f64(a.to_f64() + b.to_f64()),
                        OpCode::SubFloat => VmValue::from_f64(a.to_f64() - b.to_f64()),
                        OpCode::MulFloat => VmValue::from_f64(a.to_f64() * b.to_f64()),
                        OpCode::DivFloat => {
                            let bv = b.to_f64();
                            if bv == 0.0 {
                                return Err(crate::error::RuntimeError::new("division by zero"));
                            }
                            VmValue::from_f64(a.to_f64() / bv)
                        }
                        OpCode::ModFloat => {
                            let bv = b.to_f64();
                            if bv == 0.0 {
                                return Err(crate::error::RuntimeError::new("modulo by zero"));
                            }
                            VmValue::from_f64(a.to_f64() % bv)
                        }
                        OpCode::PowFloat => VmValue::from_f64(a.to_f64().powf(b.to_f64())),
                        _ => unreachable!(),
                    }
                } else {
                    let generic_op = match op {
                        OpCode::AddFloat => OpCode::Add,
                        OpCode::SubFloat => OpCode::Sub,
                        OpCode::MulFloat => OpCode::Mul,
                        OpCode::DivFloat => OpCode::Div,
                        OpCode::ModFloat => OpCode::Mod,
                        OpCode::PowFloat => OpCode::Pow,
                        _ => unreachable!(),
                    };
                    self.exec_arith(generic_op, a, b)?
                };
                self.stack[base + first_reg] = res;
            }
            OpCode::LtFloat
            | OpCode::GtFloat
            | OpCode::LteFloat
            | OpCode::GteFloat
            | OpCode::EqFloat
            | OpCode::NeqFloat => {
                let w1 = code[*ip];
                *ip += 1;
                let (src1, src2) = (hi(w1), lo(w1));
                let a = self.stack[base + src1];
                let b = self.stack[base + src2];
                let res = if (a.is_f64() || a.is_int()) && (b.is_f64() || b.is_int()) {
                    let cmp_res = match op {
                        OpCode::LtFloat => a.to_f64() < b.to_f64(),
                        OpCode::GtFloat => a.to_f64() > b.to_f64(),
                        OpCode::LteFloat => a.to_f64() <= b.to_f64(),
                        OpCode::GteFloat => a.to_f64() >= b.to_f64(),
                        OpCode::EqFloat => a.to_f64() == b.to_f64(),
                        OpCode::NeqFloat => a.to_f64() != b.to_f64(),
                        _ => unreachable!(),
                    };
                    VmValue::from_bool(cmp_res)
                } else {
                    let generic_op = match op {
                        OpCode::LtFloat => OpCode::Lt,
                        OpCode::GtFloat => OpCode::Gt,
                        OpCode::LteFloat => OpCode::Lte,
                        OpCode::GteFloat => OpCode::Gte,
                        OpCode::EqFloat => OpCode::Eq,
                        OpCode::NeqFloat => OpCode::Neq,
                        _ => unreachable!(),
                    };
                    self.exec_cmp(generic_op, a, b)
                };
                self.stack[base + first_reg] = res;
            }
            OpCode::Eq | OpCode::Neq | OpCode::Lt | OpCode::Lte | OpCode::Gt | OpCode::Gte => {
                let w1 = code[*ip];
                *ip += 1;
                let (src1, src2) = (hi(w1), lo(w1));
                let a = self.stack[base + src1];
                let b = self.stack[base + src2];
                let r = self.exec_cmp(op, a, b);
                self.stack[base + first_reg] = r;
            }
            OpCode::ToString => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack[base + src];
                let s = self.heap.str_repr(v);
                self.stack[base + first_reg] = self.heap.alloc_str(s);
            }
            OpCode::StrConcat => {
                let w1 = code[*ip];
                *ip += 1;
                let (src1, src2) = (hi(w1), lo(w1));
                let a = self.stack[base + src1];
                let b = self.stack[base + src2];
                let sa = self.heap.str_repr(a);
                let sb = self.heap.str_repr(b);
                let combined = format!("{sa}{sb}");
                self.stack[base + first_reg] = self.heap.alloc_str(combined);
            }
            OpCode::BuildStr => {
                let count = hi(code[*ip]);
                *ip += 1;

                let parts: Vec<String> = (0..count)
                    .map(|i| {
                        let reg_idx = hi(code[*ip + i]);
                        self.heap.str_repr(self.stack[base + reg_idx])
                    })
                    .collect();
                *ip += count;
                let total_len: usize = parts.iter().map(|s| s.len()).sum();
                let mut combined = String::with_capacity(total_len);
                for p in &parts {
                    combined.push_str(p);
                }
                self.stack[base + first_reg] = self.heap.alloc_str(combined);
            }
            OpCode::StrLength => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack[base + src];
                let len = self.exec_str_length(v)?;
                self.stack[base + first_reg] = len;
            }
            OpCode::StrSlice => {
                let w1 = code[*ip];
                *ip += 1;
                let (src1, src2) = (hi(w1), lo(w1));
                let s = self.stack[base + src1];
                let idx = self.stack[base + src2];
                self.stack[base + first_reg] = self.exec_str_slice(s, idx)?;
            }
            _ => return Ok(false),
        }

        Ok(true)
    }
}
