use crate::closure::VmClosure;
use crate::error::VmResult;
use crate::exec::arith;
use crate::exec::compare;
use crate::exec::ctx::ExecCtx;
use crate::frame_store::FrameStore;
use crate::value::VmValue;
use varn_core::OpCode;

use super::{hi, lo};

/// The `integer overflow` error for the typed int opcodes. Cold and outlined
/// so the check on the hot path is a compare and a never-taken branch.
#[cold]
#[inline(never)]
fn int_overflow(op: &str, a: i64, b: i64) -> crate::error::RuntimeError {
    crate::error::RuntimeError::integer_overflow(format!(
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
        // Lectura boxeada (forma genérica): mismo tráfico que el stack
        // universal anterior. Las vías rápidas por clase la evitan.
        let read_binary_operands =
            |code: &[u16], ip: &mut usize, store: &FrameStore, base: usize| -> (VmValue, VmValue) {
                let w1 = code[*ip];
                *ip += 1;
                (store.box_reg(base, hi(w1)), store.box_reg(base, lo(w1)))
            };
        // Escritura con conversión a la clase del destino.
        macro_rules! w {
            ($dst:expr, $v:expr) => {
                self.stack.unbox_into_reg(base, $dst, $v)?
            };
        }

        match op {
            // Generic arithmetic
            OpCode::Add => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                let r = arith::add(a, b, &mut self.heap)?;
                w!(first_reg, r);
            }
            OpCode::Sub => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                let r = arith::sub(a, b, &mut self.heap)?;
                w!(first_reg, r);
            }
            OpCode::Mul => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                let r = arith::mul(a, b, &mut self.heap)?;
                w!(first_reg, r);
            }
            OpCode::Div => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                let r = arith::div(a, b, &mut self.heap)?;
                w!(first_reg, r);
            }
            OpCode::Mod => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                let r = arith::modulo(a, b, &mut self.heap)?;
                w!(first_reg, r);
            }
            OpCode::Pow => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                let r = arith::pow(a, b, &mut self.heap)?;
                w!(first_reg, r);
            }
            OpCode::BitAnd => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                let r = arith::bit_and(a, b, &mut self.heap);
                w!(first_reg, r);
            }
            OpCode::BitOr => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                let r = arith::bit_or(a, b, &mut self.heap);
                w!(first_reg, r);
            }
            OpCode::BitXor => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                let r = arith::bit_xor(a, b, &mut self.heap);
                w!(first_reg, r);
            }
            OpCode::Shl => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                let r = arith::shl(a, b, &mut self.heap);
                w!(first_reg, r);
            }
            OpCode::Shr => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                let r = arith::shr(a, b, &mut self.heap);
                w!(first_reg, r);
            }
            OpCode::Ushr => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                let r = arith::ushr(a, b, &mut self.heap);
                w!(first_reg, r);
            }

            // Unary operators
            OpCode::Negate => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack.box_reg(base, src);
                let r = arith::negate(v, &mut self.heap)?;
                w!(first_reg, r);
            }
            OpCode::Not => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack.box_reg(base, src);
                w!(first_reg, compare::logical_not(v));
            }

            // Immediate arithmetic
            OpCode::AddImm => {
                let w1 = code[*ip];
                *ip += 1;
                let src = hi(w1);
                let imm = lo(w1) as i8 as i64;
                // Vía rápida: fuente en GPR, sin boxeo ni tag-check.
                if self.stack.reg_class(base, src) == crate::frame_store::SlotClass::Gpr {
                    let a_val = self.stack.g(base, src);
                    // Raise on overflow, never promote to float — numeric.rs
                    // denies that promotion for int arithmetic.
                    match varn_core::add_int(a_val, imm) {
                        Some(r) => w!(first_reg, VmValue::from_int(r)),
                        None => return Err(int_overflow("+", a_val, imm)),
                    }
                } else {
                    let v = self.stack.box_reg(base, src);
                    if self.heap.is_int(v) {
                        let a_val = self.heap.as_int(v);
                        match varn_core::add_int(a_val, imm) {
                            Some(r) => w!(first_reg, VmValue::from_int(r)),
                            None => return Err(int_overflow("+", a_val, imm)),
                        };
                    } else {
                        let imm_v = VmValue::from_int(imm);
                        let r = arith::add(v, imm_v, &mut self.heap)?;
                        w!(first_reg, r);
                    }
                }
            }
            OpCode::SubImm => {
                let w1 = code[*ip];
                *ip += 1;
                let src = hi(w1);
                let imm = lo(w1) as i8 as i64;
                if self.stack.reg_class(base, src) == crate::frame_store::SlotClass::Gpr {
                    let a_val = self.stack.g(base, src);
                    match varn_core::sub_int(a_val, imm) {
                        Some(r) => w!(first_reg, VmValue::from_int(r)),
                        None => return Err(int_overflow("-", a_val, imm)),
                    }
                } else {
                    let v = self.stack.box_reg(base, src);
                    if self.heap.is_int(v) {
                        let a_val = self.heap.as_int(v);
                        match varn_core::sub_int(a_val, imm) {
                            Some(r) => w!(first_reg, VmValue::from_int(r)),
                            None => return Err(int_overflow("-", a_val, imm)),
                        };
                    } else {
                        let imm_v = VmValue::from_int(imm);
                        let r = arith::sub(v, imm_v, &mut self.heap)?;
                        w!(first_reg, r);
                    }
                }
            }

            // Integer-specialized arithmetic
            OpCode::AddInt => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    match varn_core::add_int(a_val, b_val) {
                        Some(r) => w!(first_reg, VmValue::from_int(r)),
                        None => return Err(int_overflow("+", a_val, b_val)),
                    }
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    if a.is_int() && b.is_int() {
                        let a_val = a.as_int();
                        let b_val = b.as_int();
                        match varn_core::add_int(a_val, b_val) {
                            Some(r) => w!(first_reg, VmValue::from_int(r)),
                            None => return Err(int_overflow("+", a_val, b_val)),
                        }
                    } else {
                        let r = arith::add(a, b, &mut self.heap)?;
                        w!(first_reg, r);
                    };
                }
            }
            OpCode::SubInt => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    match varn_core::sub_int(a_val, b_val) {
                        Some(r) => w!(first_reg, VmValue::from_int(r)),
                        None => return Err(int_overflow("-", a_val, b_val)),
                    }
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    if a.is_int() && b.is_int() {
                        let a_val = a.as_int();
                        let b_val = b.as_int();
                        match varn_core::sub_int(a_val, b_val) {
                            Some(r) => w!(first_reg, VmValue::from_int(r)),
                            None => return Err(int_overflow("-", a_val, b_val)),
                        }
                    } else {
                        let r = arith::sub(a, b, &mut self.heap)?;
                        w!(first_reg, r);
                    };
                }
            }
            OpCode::MulInt => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    match varn_core::mul_int(a_val, b_val) {
                        Some(r) => w!(first_reg, VmValue::from_int(r)),
                        None => return Err(int_overflow("*", a_val, b_val)),
                    }
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    if a.is_int() && b.is_int() {
                        let a_val = a.as_int();
                        let b_val = b.as_int();
                        match varn_core::mul_int(a_val, b_val) {
                            Some(r) => w!(first_reg, VmValue::from_int(r)),
                            None => return Err(int_overflow("*", a_val, b_val)),
                        }
                    } else {
                        let r = arith::mul(a, b, &mut self.heap)?;
                        w!(first_reg, r);
                    };
                }
            }
            OpCode::DivInt => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    if b_val == 0 {
                        return Err(crate::error::RuntimeError::division_by_zero("division by zero"));
                    }
                    w!(first_reg, VmValue::from_f64(a_val as f64 / b_val as f64));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    if a.is_int() && b.is_int() {
                        let a_val = a.as_int();
                        let b_val = b.as_int();
                        if b_val == 0 {
                            return Err(crate::error::RuntimeError::division_by_zero("division by zero"));
                        }
                        w!(first_reg, VmValue::from_f64(a_val as f64 / b_val as f64));
                    } else {
                        let r = arith::div(a, b, &mut self.heap)?;
                        w!(first_reg, r);
                    };
                }
            }
            OpCode::ModInt => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    let r = varn_core::rem_int(a_val, b_val)
                        .map_err(|f| arith::int_div_fault(f, "%", a_val, b_val))?;
                    w!(first_reg, VmValue::from_int(r));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    if a.is_int() && b.is_int() {
                        let a_val = a.as_int();
                        let b_val = b.as_int();
                        let r = varn_core::rem_int(a_val, b_val)
                            .map_err(|f| arith::int_div_fault(f, "%", a_val, b_val))?;
                        w!(first_reg, VmValue::from_int(r));
                    } else {
                        let r = arith::modulo(a, b, &mut self.heap)?;
                        w!(first_reg, r);
                    };
                }
            }
            OpCode::PowInt => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    if b_val < 0 {
                        return Err(crate::error::RuntimeError::new(
                            "negative exponent in integer power",
                        ));
                    }
                    let e = u32::try_from(b_val).unwrap_or(u32::MAX);
                    match varn_core::pow_int(a_val, e) {
                        Some(r) => w!(first_reg, VmValue::from_int(r)),
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
                            Some(r) => w!(first_reg, VmValue::from_int(r)),
                            None => return Err(int_overflow("**", a_val, b_val)),
                        }
                    } else {
                        let r = arith::pow(a, b, &mut self.heap)?;
                        w!(first_reg, r);
                    };
                }
            }

            // Integer comparisons
            OpCode::LtInt => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    w!(first_reg, VmValue::from_bool(a_val < b_val));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    w!(
                        first_reg,
                        VmValue::from_bool(if a.is_int() && b.is_int() {
                            a.as_int() < b.as_int()
                        } else {
                            compare::lt_heap(a, b, &self.heap)
                        })
                    );
                }
            }
            OpCode::GtInt => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    w!(first_reg, VmValue::from_bool(a_val > b_val));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    w!(
                        first_reg,
                        VmValue::from_bool(if a.is_int() && b.is_int() {
                            a.as_int() > b.as_int()
                        } else {
                            compare::gt_heap(a, b, &self.heap)
                        })
                    );
                }
            }
            OpCode::LteInt => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    w!(first_reg, VmValue::from_bool(a_val <= b_val));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    w!(
                        first_reg,
                        VmValue::from_bool(if a.is_int() && b.is_int() {
                            a.as_int() <= b.as_int()
                        } else {
                            compare::lte_heap(a, b, &self.heap)
                        })
                    );
                }
            }
            OpCode::GteInt => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    w!(first_reg, VmValue::from_bool(a_val >= b_val));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    w!(
                        first_reg,
                        VmValue::from_bool(if a.is_int() && b.is_int() {
                            a.as_int() >= b.as_int()
                        } else {
                            compare::gte_heap(a, b, &self.heap)
                        })
                    );
                }
            }
            OpCode::EqInt => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    w!(first_reg, VmValue::from_bool(a_val == b_val));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    w!(
                        first_reg,
                        VmValue::from_bool(if a.is_int() && b.is_int() {
                            a.as_int() == b.as_int()
                        } else {
                            compare::eq(a, b, &self.heap)
                        })
                    );
                }
            }
            OpCode::NeqInt => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.int_pair(base, r1, r2) {
                    w!(first_reg, VmValue::from_bool(a_val != b_val));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    w!(
                        first_reg,
                        VmValue::from_bool(if a.is_int() && b.is_int() {
                            a.as_int() != b.as_int()
                        } else {
                            compare::neq(a, b, &self.heap)
                        })
                    );
                }
            }

            // Float-specialized arithmetic
            OpCode::AddFloat => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    w!(first_reg, VmValue::from_f64(a_val + b_val));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    match float_fast(a, b, &self.heap) {
                        Some((av, bv)) => w!(first_reg, VmValue::from_f64(av + bv)),
                        None => {
                            let r = arith::add(a, b, &mut self.heap)?;
                            w!(first_reg, r);
                        }
                    };
                }
            }
            OpCode::SubFloat => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    w!(first_reg, VmValue::from_f64(a_val - b_val));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    match float_fast(a, b, &self.heap) {
                        Some((av, bv)) => w!(first_reg, VmValue::from_f64(av - bv)),
                        None => {
                            let r = arith::sub(a, b, &mut self.heap)?;
                            w!(first_reg, r);
                        }
                    };
                }
            }
            OpCode::MulFloat => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    w!(first_reg, VmValue::from_f64(a_val * b_val));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    match float_fast(a, b, &self.heap) {
                        Some((av, bv)) => w!(first_reg, VmValue::from_f64(av * bv)),
                        None => {
                            let r = arith::mul(a, b, &mut self.heap)?;
                            w!(first_reg, r);
                        }
                    };
                }
            }
            OpCode::DivFloat => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    if b_val == 0.0 {
                        return Err(crate::error::RuntimeError::division_by_zero("division by zero"));
                    }
                    w!(first_reg, VmValue::from_f64(a_val / b_val));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    match float_fast(a, b, &self.heap) {
                        Some((av, bv)) => {
                            if bv == 0.0 {
                                return Err(crate::error::RuntimeError::division_by_zero("division by zero"));
                            }
                            w!(first_reg, VmValue::from_f64(av / bv));
                        }
                        None => {
                            let r = arith::div(a, b, &mut self.heap)?;
                            w!(first_reg, r);
                        }
                    };
                }
            }
            OpCode::ModFloat => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    if b_val == 0.0 {
                        return Err(crate::error::RuntimeError::division_by_zero("modulo by zero"));
                    }
                    w!(first_reg, VmValue::from_f64(a_val % b_val));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    match float_fast(a, b, &self.heap) {
                        Some((av, bv)) => {
                            if bv == 0.0 {
                                return Err(crate::error::RuntimeError::division_by_zero("modulo by zero"));
                            }
                            w!(first_reg, VmValue::from_f64(av % bv));
                        }
                        None => {
                            let r = arith::modulo(a, b, &mut self.heap)?;
                            w!(first_reg, r);
                        }
                    };
                }
            }
            OpCode::PowFloat => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    w!(first_reg, VmValue::from_f64(a_val.powf(b_val)));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    match float_fast(a, b, &self.heap) {
                        Some((av, bv)) => w!(first_reg, VmValue::from_f64(av.powf(bv))),
                        None => {
                            let r = arith::pow(a, b, &mut self.heap)?;
                            w!(first_reg, r);
                        }
                    };
                }
            }

            // Float comparisons
            OpCode::LtFloat => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    w!(first_reg, VmValue::from_bool(a_val < b_val));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    w!(
                        first_reg,
                        VmValue::from_bool(
                            if (a.is_f64() || self.heap.is_int(a))
                                && (b.is_f64() || self.heap.is_int(b))
                            {
                                self.heap.to_f64_val(a) < self.heap.to_f64_val(b)
                            } else {
                                compare::lt_heap(a, b, &self.heap)
                            },
                        )
                    );
                }
            }
            OpCode::GtFloat => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    w!(first_reg, VmValue::from_bool(a_val > b_val));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    w!(
                        first_reg,
                        VmValue::from_bool(
                            if (a.is_f64() || self.heap.is_int(a))
                                && (b.is_f64() || self.heap.is_int(b))
                            {
                                self.heap.to_f64_val(a) > self.heap.to_f64_val(b)
                            } else {
                                compare::gt_heap(a, b, &self.heap)
                            },
                        )
                    );
                }
            }
            OpCode::LteFloat => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    w!(first_reg, VmValue::from_bool(a_val <= b_val));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    w!(
                        first_reg,
                        VmValue::from_bool(
                            if (a.is_f64() || self.heap.is_int(a))
                                && (b.is_f64() || self.heap.is_int(b))
                            {
                                self.heap.to_f64_val(a) <= self.heap.to_f64_val(b)
                            } else {
                                compare::lte_heap(a, b, &self.heap)
                            },
                        )
                    );
                }
            }
            OpCode::GteFloat => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    w!(first_reg, VmValue::from_bool(a_val >= b_val));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    w!(
                        first_reg,
                        VmValue::from_bool(
                            if (a.is_f64() || self.heap.is_int(a))
                                && (b.is_f64() || self.heap.is_int(b))
                            {
                                self.heap.to_f64_val(a) >= self.heap.to_f64_val(b)
                            } else {
                                compare::gte_heap(a, b, &self.heap)
                            },
                        )
                    );
                }
            }
            OpCode::EqFloat => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    w!(first_reg, VmValue::from_bool(a_val == b_val));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    w!(
                        first_reg,
                        VmValue::from_bool(
                            if (a.is_f64() || self.heap.is_int(a))
                                && (b.is_f64() || self.heap.is_int(b))
                            {
                                self.heap.to_f64_val(a) == self.heap.to_f64_val(b)
                            } else {
                                compare::eq(a, b, &self.heap)
                            },
                        )
                    );
                }
            }
            OpCode::NeqFloat => {
                let w1 = code[*ip];
                *ip += 1;
                let (r1, r2) = (hi(w1), lo(w1));
                if let Some((a_val, b_val)) = self.stack.float_pair(base, r1, r2) {
                    w!(first_reg, VmValue::from_bool(a_val != b_val));
                } else {
                    let a = self.stack.box_reg(base, r1);
                    let b = self.stack.box_reg(base, r2);
                    w!(
                        first_reg,
                        VmValue::from_bool(
                            if (a.is_f64() || self.heap.is_int(a))
                                && (b.is_f64() || self.heap.is_int(b))
                            {
                                self.heap.to_f64_val(a) != self.heap.to_f64_val(b)
                            } else {
                                compare::neq(a, b, &self.heap)
                            },
                        )
                    );
                }
            }

            // Generic comparisons
            OpCode::Eq => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                w!(first_reg, VmValue::from_bool(compare::eq(a, b, &self.heap)));
            }
            OpCode::Neq => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                w!(
                    first_reg,
                    VmValue::from_bool(compare::neq(a, b, &self.heap))
                );
            }
            OpCode::Lt => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                w!(
                    first_reg,
                    VmValue::from_bool(compare::lt_heap(a, b, &self.heap))
                );
            }
            OpCode::Lte => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                w!(
                    first_reg,
                    VmValue::from_bool(compare::lte_heap(a, b, &self.heap))
                );
            }
            OpCode::Gt => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                w!(
                    first_reg,
                    VmValue::from_bool(compare::gt_heap(a, b, &self.heap))
                );
            }
            OpCode::Gte => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                w!(
                    first_reg,
                    VmValue::from_bool(compare::gte_heap(a, b, &self.heap))
                );
            }

            // String operations
            OpCode::ToString => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack.box_reg(base, src);
                let r = crate::exec::strings::to_string(v, &mut self.heap);
                w!(first_reg, r);
            }
            OpCode::StrConcat => {
                let (a, b) = read_binary_operands(code, ip, &self.stack, base);
                let r = crate::exec::strings::str_concat(a, b, &mut self.heap);
                w!(first_reg, r);
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
                w!(first_reg, r);
            }
            OpCode::StrLength => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack.box_reg(base, src);
                let r = self.exec_str_length(v)?;
                w!(first_reg, r);
            }
            OpCode::StrSlice => {
                let (s, idx) = read_binary_operands(code, ip, &self.stack, base);
                let r = self.exec_str_slice(s, idx)?;
                w!(first_reg, r);
            }
            _ => return Ok(false),
        }

        Ok(true)
    }
}

/// Camino mixto float de los opcodes `*Float`: `Some((x, y))` si ambos
/// operandos son numéricos (f64 o int con ensanchado), `None` si el llamante
/// debe aplicar su fallback genérico (`arith::*`, p. ej. concat de strs).
#[inline(always)]
fn float_fast(a: VmValue, b: VmValue, heap: &crate::heap::Heap) -> Option<(f64, f64)> {
    if a.is_f64() && b.is_f64() {
        Some((a.as_f64(), b.as_f64()))
    } else if (a.is_f64() || heap.is_int(a)) && (b.is_f64() || heap.is_int(b)) {
        Some((mixed_to_f64(a, heap), mixed_to_f64(b, heap)))
    } else {
        None
    }
}

#[inline(always)]
fn mixed_to_f64(v: VmValue, heap: &crate::heap::Heap) -> f64 {
    if v.is_f64() {
        v.as_f64()
    } else {
        heap.to_f64_val(v)
    }
}
