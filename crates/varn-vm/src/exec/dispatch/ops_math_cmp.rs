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
                if self.heap.is_int(v) {
                    let (r, overflow) = self.heap.as_int(v).overflowing_add(imm);
                    self.stack[base + first_reg] = if overflow {
                        VmValue::from_f64(self.heap.as_int(v) as f64 + imm as f64)
                    } else {
                        self.heap.make_int(r)
                    };
                } else {
                    let imm_v = self.heap.make_int(imm);
                    self.stack[base + first_reg] =
                        crate::exec::arith::add(v, imm_v, &mut self.heap)?;
                }
            }
            OpCode::SubImm => {
                let w1 = code[*ip];
                *ip += 1;
                let src = hi(w1);
                let imm = lo(w1) as i8 as i64;
                let v = self.stack[base + src];
                if self.heap.is_int(v) {
                    let (r, overflow) = self.heap.as_int(v).overflowing_sub(imm);
                    self.stack[base + first_reg] = if overflow {
                        VmValue::from_f64(self.heap.as_int(v) as f64 - imm as f64)
                    } else {
                        self.heap.make_int(r)
                    };
                } else {
                    let imm_v = self.heap.make_int(imm);
                    self.stack[base + first_reg] =
                        crate::exec::arith::sub(v, imm_v, &mut self.heap)?;
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
                let res = if self.heap.is_int(a) && self.heap.is_int(b) {
                    let a_val = self.heap.as_int(a);
                    let b_val = self.heap.as_int(b);
                    match op {
                        OpCode::AddInt => self.heap.make_int(a_val.wrapping_add(b_val)),
                        OpCode::SubInt => self.heap.make_int(a_val.wrapping_sub(b_val)),
                        OpCode::MulInt => self.heap.make_int(a_val.wrapping_mul(b_val)),
                        OpCode::DivInt => {
                            if b_val == 0 {
                                return Err(crate::error::RuntimeError::new("division by zero"));
                            }
                            VmValue::from_f64(a_val as f64 / b_val as f64)
                        }
                        OpCode::ModInt => {
                            if b_val == 0 {
                                return Err(crate::error::RuntimeError::new("modulo by zero"));
                            }
                            self.heap.make_int(a_val % b_val)
                        }
                        OpCode::PowInt => {
                            if b_val < 0 {
                                return Err(crate::error::RuntimeError::new(
                                    "negative exponent in integer power",
                                ));
                            }
                            let e = u32::try_from(b_val).unwrap_or(u32::MAX);
                            self.heap.make_int(a_val.wrapping_pow(e))
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
                let res = if self.heap.is_int(a) && self.heap.is_int(b) {
                    let a_val = self.heap.as_int(a);
                    let b_val = self.heap.as_int(b);
                    let cmp_res = match op {
                        OpCode::LtInt => a_val < b_val,
                        OpCode::GtInt => a_val > b_val,
                        OpCode::LteInt => a_val <= b_val,
                        OpCode::GteInt => a_val >= b_val,
                        OpCode::EqInt => a_val == b_val,
                        OpCode::NeqInt => a_val != b_val,
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
                let res = if (a.is_f64() || self.heap.is_int(a)) && (b.is_f64() || self.heap.is_int(b)) {
                    match op {
                        OpCode::AddFloat => VmValue::from_f64(self.heap.to_f64_val(a) + self.heap.to_f64_val(b)),
                        OpCode::SubFloat => VmValue::from_f64(self.heap.to_f64_val(a) - self.heap.to_f64_val(b)),
                        OpCode::MulFloat => VmValue::from_f64(self.heap.to_f64_val(a) * self.heap.to_f64_val(b)),
                        OpCode::DivFloat => {
                            let bv = self.heap.to_f64_val(b);
                            if bv == 0.0 {
                                return Err(crate::error::RuntimeError::new("division by zero"));
                            }
                            VmValue::from_f64(self.heap.to_f64_val(a) / bv)
                        }
                        OpCode::ModFloat => {
                            let bv = self.heap.to_f64_val(b);
                            if bv == 0.0 {
                                return Err(crate::error::RuntimeError::new("modulo by zero"));
                            }
                            VmValue::from_f64(self.heap.to_f64_val(a) % bv)
                        }
                        OpCode::PowFloat => VmValue::from_f64(self.heap.to_f64_val(a).powf(self.heap.to_f64_val(b))),
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
                let res = if (a.is_f64() || self.heap.is_int(a)) && (b.is_f64() || self.heap.is_int(b)) {
                    let cmp_res = match op {
                        OpCode::LtFloat => self.heap.to_f64_val(a) < self.heap.to_f64_val(b),
                        OpCode::GtFloat => self.heap.to_f64_val(a) > self.heap.to_f64_val(b),
                        OpCode::LteFloat => self.heap.to_f64_val(a) <= self.heap.to_f64_val(b),
                        OpCode::GteFloat => self.heap.to_f64_val(a) >= self.heap.to_f64_val(b),
                        OpCode::EqFloat => self.heap.to_f64_val(a) == self.heap.to_f64_val(b),
                        OpCode::NeqFloat => self.heap.to_f64_val(a) != self.heap.to_f64_val(b),
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
                // `alloc_str_dynamic`, not `alloc_str`: a coerced value is a
                // runtime-produced string, so the interner probe can only ever
                // hash its full contents and miss. See that method's contract.
                self.stack[base + first_reg] = self.heap.alloc_str_dynamic(&s);
            }
            OpCode::StrConcat => {
                let w1 = code[*ip];
                *ip += 1;
                let (src1, src2) = (hi(w1), lo(w1));
                let a = self.stack[base + src1];
                let b = self.stack[base + src2];
                // Append both operands into ONE buffer. The `format!` this
                // replaces allocated a third String on top of the two
                // `str_repr` results, to produce a value that is then copied
                // again into the heap string.
                let mut combined = String::new();
                self.heap.str_repr_into(a, &mut combined);
                self.heap.str_repr_into(b, &mut combined);
                self.stack[base + first_reg] = self.heap.alloc_str_dynamic(&combined);
            }
            OpCode::BuildStr => {
                let count = hi(code[*ip]);
                *ip += 1;

                // Append each part directly into one buffer: strings borrow,
                // ints/bools/null format in place — no per-part String alloc.
                let mut combined = String::new();
                for i in 0..count {
                    let reg_idx = hi(code[*ip + i]);
                    self.heap
                        .str_repr_into(self.stack[base + reg_idx], &mut combined);
                }
                *ip += count;
                self.stack[base + first_reg] = self.heap.alloc_str_dynamic(&combined);
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
