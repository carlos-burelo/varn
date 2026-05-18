use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::exec::dispatch::ControlSignal;
use crate::value::VmValue;

impl ExecCtx {
    #[inline(always)]
    pub(super) fn op_add(&mut self) -> VmResult<ControlSignal> {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let r = crate::exec::arith::add(a, b, &mut self.heap)?;
        self.stack.push(r);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_sub(&mut self) -> VmResult<ControlSignal> {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let r = crate::exec::arith::sub(a, b, &mut self.heap)?;
        self.stack.push(r);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_mul(&mut self) -> VmResult<ControlSignal> {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let r = crate::exec::arith::mul(a, b, &mut self.heap)?;
        self.stack.push(r);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_div(&mut self) -> VmResult<ControlSignal> {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let r = crate::exec::arith::div(a, b, &mut self.heap)?;
        self.stack.push(r);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_add_i32(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        self.stack.push(VmValue::from_i32(a.as_i32() + b.as_i32()));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_sub_i32(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        self.stack.push(VmValue::from_i32(a.as_i32() - b.as_i32()));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_mul_i32(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        self.stack.push(VmValue::from_i32(a.as_i32() * b.as_i32()));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_eq(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        self.stack.push(VmValue::from_bool(crate::exec::compare::eq(
            a, b, &self.heap,
        )));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_neq(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        self.stack
            .push(VmValue::from_bool(crate::exec::compare::neq(
                a, b, &self.heap,
            )));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_lt(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let res = crate::exec::compare::lt_heap(a, b, &self.heap);
        self.stack.push(VmValue::from_bool(res));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_lte(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let res = crate::exec::compare::lte_heap(a, b, &self.heap);
        self.stack.push(VmValue::from_bool(res));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_gt(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let res = crate::exec::compare::gt_heap(a, b, &self.heap);
        self.stack.push(VmValue::from_bool(res));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_gte(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let res = crate::exec::compare::gte_heap(a, b, &self.heap);
        self.stack.push(VmValue::from_bool(res));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_div_i32(&mut self) -> VmResult<ControlSignal> {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let r = crate::exec::arith::div_i32(a, b)?;
        self.stack.push(r);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_mod(&mut self) -> VmResult<ControlSignal> {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let r = crate::exec::arith::modulo(a, b, &mut self.heap)?;
        self.stack.push(r);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_pow(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let r = crate::exec::arith::pow(a, b);
        self.stack.push(r);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_negate(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let a = self.stack[len - 1];
        unsafe { self.stack.set_len(len - 1) };
        let r = crate::exec::arith::negate(a, &mut self.heap);
        self.stack.push(r);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_bit_and(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let r = crate::exec::arith::bit_and(a, b);
        self.stack.push(r);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_bit_or(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let r = crate::exec::arith::bit_or(a, b);
        self.stack.push(r);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_bit_xor(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let r = crate::exec::arith::bit_xor(a, b);
        self.stack.push(r);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_shl(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let r = crate::exec::arith::shl(a, b);
        self.stack.push(r);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_shr(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let r = crate::exec::arith::shr(a, b);
        self.stack.push(r);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_ushr(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let r = crate::exec::arith::ushr(a, b);
        self.stack.push(r);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_not(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let a = self.stack[len - 1];
        unsafe { self.stack.set_len(len - 1) };
        let r = crate::exec::compare::logical_not(a);
        self.stack.push(r);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_sub_f64(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        self.stack.push(crate::exec::arith::sub_f64(a, b));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_mul_f64(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        self.stack.push(crate::exec::arith::mul_f64(a, b));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_div_f64(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        self.stack.push(crate::exec::arith::div_f64(a, b));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_to_string(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let a = unsafe { *self.stack.get_unchecked(len - 1) };
        unsafe { self.stack.set_len(len - 1) };
        let s = crate::exec::strings::to_string(a, &mut self.heap);
        self.stack.push(s);
        ControlSignal::ContinueInstruction
    }
    #[inline(always)]
    pub(super) fn op_add_f64(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        self.stack.push(crate::exec::arith::add_f64(a, b));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_eq_i32(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        self.stack
            .push(VmValue::from_bool(a.as_i32() == b.as_i32()));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_eq_f64(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        self.stack
            .push(VmValue::from_bool(a.as_f64() == b.as_f64()));
        ControlSignal::ContinueInstruction
    }
}
