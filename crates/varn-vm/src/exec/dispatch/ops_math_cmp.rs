use crate::closure::VmClosure;
use crate::error::VmResult;
use crate::exec::arith;
use crate::exec::compare;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_core::OpCode;

use super::{hi, lo};

/// The `integer overflow` error for the typed int opcodes. Cold and outlined
/// so the check on the hot path is a compare and a never-taken branch.
#[cold]
#[inline(never)]
fn int_overflow(op: &str, a: i64, b: i64) -> crate::error::RuntimeError {
    crate::error::RuntimeError::new(format!(
        "integer overflow: {a} {op} {b} is outside int ({}..={})",
        varn_core::INT_MIN,
        varn_core::INT_MAX
    ))
}

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
        let read_binary_operands = |code: &[u16], ip: &mut usize, stack: &[VmValue]| -> (VmValue, VmValue) {
            let w1 = code[*ip];
            *ip += 1;
            (stack[base + hi(w1)], stack[base + lo(w1)])
        };

        match op {
            // Generic arithmetic
            OpCode::Add => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = arith::add(a, b, &mut self.heap)?;
            }
            OpCode::Sub => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = arith::sub(a, b, &mut self.heap)?;
            }
            OpCode::Mul => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = arith::mul(a, b, &mut self.heap)?;
            }
            OpCode::Div => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = arith::div(a, b, &mut self.heap)?;
            }
            OpCode::Mod => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = arith::modulo(a, b, &mut self.heap)?;
            }
            OpCode::Pow => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = arith::pow(a, b, &mut self.heap)?;
            }
            OpCode::BitAnd => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = arith::bit_and(a, b, &mut self.heap);
            }
            OpCode::BitOr => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = arith::bit_or(a, b, &mut self.heap);
            }
            OpCode::BitXor => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = arith::bit_xor(a, b, &mut self.heap);
            }
            OpCode::Shl => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = arith::shl(a, b, &mut self.heap);
            }
            OpCode::Shr => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = arith::shr(a, b, &mut self.heap);
            }
            OpCode::Ushr => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = arith::ushr(a, b, &mut self.heap);
            }

            // Unary operators
            OpCode::Negate => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack[base + src];
                self.stack[base + first_reg] = arith::negate(v, &mut self.heap)?;
            }
            OpCode::Not => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack[base + src];
                self.stack[base + first_reg] = compare::logical_not(v);
            }

            // Immediate arithmetic
            OpCode::AddImm => {
                let w1 = code[*ip];
                *ip += 1;
                let src = hi(w1);
                let imm = lo(w1) as i8 as i64;
                let v = self.stack[base + src];
                if self.heap.is_int(v) {
                    // The old `overflowing_add` guard here tested i64 overflow and
                    // promoted to float. Both were wrong: an i48 operand plus an
                    // i8 immediate cannot overflow i64, so the branch was dead,
                    // and float promotion is exactly what numeric.rs denies.
                    let a_val = self.heap.as_int(v);
                    self.stack[base + first_reg] = match varn_core::add_i48(a_val, imm) {
                        Some(r) => VmValue::from_int(r),
                        None => return Err(int_overflow("+", a_val, imm)),
                    };
                } else {
                    let imm_v = VmValue::from_int(imm);
                    self.stack[base + first_reg] = arith::add(v, imm_v, &mut self.heap)?;
                }
            }
            OpCode::SubImm => {
                let w1 = code[*ip];
                *ip += 1;
                let src = hi(w1);
                let imm = lo(w1) as i8 as i64;
                let v = self.stack[base + src];
                if self.heap.is_int(v) {
                    // The old `overflowing_sub` guard here tested i64 overflow and
                    // promoted to float. Both were wrong: an i48 operand plus an
                    // i8 immediate cannot overflow i64, so the branch was dead,
                    // and float promotion is exactly what numeric.rs denies.
                    let a_val = self.heap.as_int(v);
                    self.stack[base + first_reg] = match varn_core::sub_i48(a_val, imm) {
                        Some(r) => VmValue::from_int(r),
                        None => return Err(int_overflow("-", a_val, imm)),
                    };
                } else {
                    let imm_v = VmValue::from_int(imm);
                    self.stack[base + first_reg] = arith::sub(v, imm_v, &mut self.heap)?;
                }
            }

            // Integer-specialized arithmetic
            OpCode::AddInt => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = if self.heap.is_int(a) && self.heap.is_int(b) {
                    let a_val = self.heap.as_int(a);
                    let b_val = self.heap.as_int(b);
                    match varn_core::add_i48(a_val, b_val) {
                        Some(r) => VmValue::from_int(r),
                        None => return Err(int_overflow("+", a_val, b_val)),
                    }
                } else {
                    arith::add(a, b, &mut self.heap)?
                };
            }
            OpCode::SubInt => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = if self.heap.is_int(a) && self.heap.is_int(b) {
                    let a_val = self.heap.as_int(a);
                    let b_val = self.heap.as_int(b);
                    match varn_core::sub_i48(a_val, b_val) {
                        Some(r) => VmValue::from_int(r),
                        None => return Err(int_overflow("-", a_val, b_val)),
                    }
                } else {
                    arith::sub(a, b, &mut self.heap)?
                };
            }
            OpCode::MulInt => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = if self.heap.is_int(a) && self.heap.is_int(b) {
                    let a_val = self.heap.as_int(a);
                    let b_val = self.heap.as_int(b);
                    match varn_core::mul_i48(a_val, b_val) {
                        Some(r) => VmValue::from_int(r),
                        None => return Err(int_overflow("*", a_val, b_val)),
                    }
                } else {
                    arith::mul(a, b, &mut self.heap)?
                };
            }
            OpCode::DivInt => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = if self.heap.is_int(a) && self.heap.is_int(b) {
                    let a_val = self.heap.as_int(a);
                    let b_val = self.heap.as_int(b);
                    if b_val == 0 {
                        return Err(crate::error::RuntimeError::new("division by zero"));
                    }
                    VmValue::from_f64(a_val as f64 / b_val as f64)
                } else {
                    arith::div(a, b, &mut self.heap)?
                };
            }
            OpCode::ModInt => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = if self.heap.is_int(a) && self.heap.is_int(b) {
                    let a_val = self.heap.as_int(a);
                    let b_val = self.heap.as_int(b);
                    if b_val == 0 {
                        return Err(crate::error::RuntimeError::new("modulo by zero"));
                    }
                    self.heap.make_int(a_val % b_val)
                } else {
                    arith::modulo(a, b, &mut self.heap)?
                };
            }
            OpCode::PowInt => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = if self.heap.is_int(a) && self.heap.is_int(b) {
                    let a_val = self.heap.as_int(a);
                    let b_val = self.heap.as_int(b);
                    if b_val < 0 {
                        return Err(crate::error::RuntimeError::new(
                            "negative exponent in integer power",
                        ));
                    }
                    let e = u32::try_from(b_val).unwrap_or(u32::MAX);
                    match varn_core::pow_i48(a_val, e) {
                        Some(r) => VmValue::from_int(r),
                        None => return Err(int_overflow("**", a_val, b_val)),
                    }
                } else {
                    arith::pow(a, b, &mut self.heap)?
                };
            }

            // Integer comparisons
            OpCode::LtInt => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = VmValue::from_bool(if self.heap.is_int(a) && self.heap.is_int(b) {
                    self.heap.as_int(a) < self.heap.as_int(b)
                } else {
                    compare::lt_heap(a, b, &self.heap)
                });
            }
            OpCode::GtInt => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = VmValue::from_bool(if self.heap.is_int(a) && self.heap.is_int(b) {
                    self.heap.as_int(a) > self.heap.as_int(b)
                } else {
                    compare::gt_heap(a, b, &self.heap)
                });
            }
            OpCode::LteInt => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = VmValue::from_bool(if self.heap.is_int(a) && self.heap.is_int(b) {
                    self.heap.as_int(a) <= self.heap.as_int(b)
                } else {
                    compare::lte_heap(a, b, &self.heap)
                });
            }
            OpCode::GteInt => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = VmValue::from_bool(if self.heap.is_int(a) && self.heap.is_int(b) {
                    self.heap.as_int(a) >= self.heap.as_int(b)
                } else {
                    compare::gte_heap(a, b, &self.heap)
                });
            }
            OpCode::EqInt => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = VmValue::from_bool(if self.heap.is_int(a) && self.heap.is_int(b) {
                    self.heap.as_int(a) == self.heap.as_int(b)
                } else {
                    compare::eq(a, b, &self.heap)
                });
            }
            OpCode::NeqInt => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = VmValue::from_bool(if self.heap.is_int(a) && self.heap.is_int(b) {
                    self.heap.as_int(a) != self.heap.as_int(b)
                } else {
                    compare::neq(a, b, &self.heap)
                });
            }

            // Float-specialized arithmetic
            OpCode::AddFloat => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = if (a.is_f64() || self.heap.is_int(a))
                    && (b.is_f64() || self.heap.is_int(b))
                {
                    VmValue::from_f64(self.heap.to_f64_val(a) + self.heap.to_f64_val(b))
                } else {
                    arith::add(a, b, &mut self.heap)?
                };
            }
            OpCode::SubFloat => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = if (a.is_f64() || self.heap.is_int(a))
                    && (b.is_f64() || self.heap.is_int(b))
                {
                    VmValue::from_f64(self.heap.to_f64_val(a) - self.heap.to_f64_val(b))
                } else {
                    arith::sub(a, b, &mut self.heap)?
                };
            }
            OpCode::MulFloat => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = if (a.is_f64() || self.heap.is_int(a))
                    && (b.is_f64() || self.heap.is_int(b))
                {
                    VmValue::from_f64(self.heap.to_f64_val(a) * self.heap.to_f64_val(b))
                } else {
                    arith::mul(a, b, &mut self.heap)?
                };
            }
            OpCode::DivFloat => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = if (a.is_f64() || self.heap.is_int(a))
                    && (b.is_f64() || self.heap.is_int(b))
                {
                    let bv = self.heap.to_f64_val(b);
                    if bv == 0.0 {
                        return Err(crate::error::RuntimeError::new("division by zero"));
                    }
                    VmValue::from_f64(self.heap.to_f64_val(a) / bv)
                } else {
                    arith::div(a, b, &mut self.heap)?
                };
            }
            OpCode::ModFloat => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = if (a.is_f64() || self.heap.is_int(a))
                    && (b.is_f64() || self.heap.is_int(b))
                {
                    let bv = self.heap.to_f64_val(b);
                    if bv == 0.0 {
                        return Err(crate::error::RuntimeError::new("modulo by zero"));
                    }
                    VmValue::from_f64(self.heap.to_f64_val(a) % bv)
                } else {
                    arith::modulo(a, b, &mut self.heap)?
                };
            }
            OpCode::PowFloat => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = if (a.is_f64() || self.heap.is_int(a))
                    && (b.is_f64() || self.heap.is_int(b))
                {
                    VmValue::from_f64(self.heap.to_f64_val(a).powf(self.heap.to_f64_val(b)))
                } else {
                    arith::pow(a, b, &mut self.heap)?
                };
            }

            // Float comparisons
            OpCode::LtFloat => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = VmValue::from_bool(if (a.is_f64() || self.heap.is_int(a))
                    && (b.is_f64() || self.heap.is_int(b))
                {
                    self.heap.to_f64_val(a) < self.heap.to_f64_val(b)
                } else {
                    compare::lt_heap(a, b, &self.heap)
                });
            }
            OpCode::GtFloat => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = VmValue::from_bool(if (a.is_f64() || self.heap.is_int(a))
                    && (b.is_f64() || self.heap.is_int(b))
                {
                    self.heap.to_f64_val(a) > self.heap.to_f64_val(b)
                } else {
                    compare::gt_heap(a, b, &self.heap)
                });
            }
            OpCode::LteFloat => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = VmValue::from_bool(if (a.is_f64() || self.heap.is_int(a))
                    && (b.is_f64() || self.heap.is_int(b))
                {
                    self.heap.to_f64_val(a) <= self.heap.to_f64_val(b)
                } else {
                    compare::lte_heap(a, b, &self.heap)
                });
            }
            OpCode::GteFloat => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = VmValue::from_bool(if (a.is_f64() || self.heap.is_int(a))
                    && (b.is_f64() || self.heap.is_int(b))
                {
                    self.heap.to_f64_val(a) >= self.heap.to_f64_val(b)
                } else {
                    compare::gte_heap(a, b, &self.heap)
                });
            }
            OpCode::EqFloat => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = VmValue::from_bool(if (a.is_f64() || self.heap.is_int(a))
                    && (b.is_f64() || self.heap.is_int(b))
                {
                    self.heap.to_f64_val(a) == self.heap.to_f64_val(b)
                } else {
                    compare::eq(a, b, &self.heap)
                });
            }
            OpCode::NeqFloat => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = VmValue::from_bool(if (a.is_f64() || self.heap.is_int(a))
                    && (b.is_f64() || self.heap.is_int(b))
                {
                    self.heap.to_f64_val(a) != self.heap.to_f64_val(b)
                } else {
                    compare::neq(a, b, &self.heap)
                });
            }

            // Generic comparisons
            OpCode::Eq => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = VmValue::from_bool(compare::eq(a, b, &self.heap));
            }
            OpCode::Neq => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = VmValue::from_bool(compare::neq(a, b, &self.heap));
            }
            OpCode::Lt => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = VmValue::from_bool(compare::lt_heap(a, b, &self.heap));
            }
            OpCode::Lte => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = VmValue::from_bool(compare::lte_heap(a, b, &self.heap));
            }
            OpCode::Gt => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = VmValue::from_bool(compare::gt_heap(a, b, &self.heap));
            }
            OpCode::Gte => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = VmValue::from_bool(compare::gte_heap(a, b, &self.heap));
            }

            // String operations
            OpCode::ToString => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack[base + src];
                self.stack[base + first_reg] = crate::exec::strings::to_string(v, &mut self.heap);
            }
            OpCode::StrConcat => {
                let (a, b) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] =
                    crate::exec::strings::str_concat(a, b, &mut self.heap);
            }
            OpCode::BuildStr => {
                let count = hi(code[*ip]);
                *ip += 1;
                let mut out = crate::strbuf::StrBuf::new();
                for i in 0..count {
                    let reg_idx = hi(code[*ip + i]);
                    self.heap
                        .str_repr_into(self.stack[base + reg_idx], &mut out);
                }
                *ip += count;
                self.stack[base + first_reg] = self.heap.alloc_str_dynamic(out.as_str());
            }
            OpCode::StrLength => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack[base + src];
                self.stack[base + first_reg] = self.exec_str_length(v)?;
            }
            OpCode::StrSlice => {
                let (s, idx) = read_binary_operands(code, ip, &self.stack);
                self.stack[base + first_reg] = self.exec_str_slice(s, idx)?;
            }
            _ => return Ok(false),
        }

        Ok(true)
    }
}
