use crate::exec::ctx::ExecCtx;
use crate::exec::dispatch::ControlSignal;
use crate::frame::VmClosure;
use crate::value::VmValue;

impl ExecCtx {
    #[inline(always)]
    pub(super) fn op_push_const(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        closure: &VmClosure,
    ) -> ControlSignal {
        let idx = code[*ip] as usize;
        *ip += 1;
        let nv = closure.constants[idx];
        self.stack.push(nv);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_push_null(&mut self) -> ControlSignal {
        self.stack.push(VmValue::null());
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_push_true(&mut self) -> ControlSignal {
        self.stack.push(VmValue::bool_true());
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_push_false(&mut self) -> ControlSignal {
        self.stack.push(VmValue::bool_false());
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_push_int16(&mut self, code: &[u16], ip: &mut usize) -> ControlSignal {
        let val = code[*ip] as i16;
        *ip += 1;
        self.stack.push(VmValue::from_i32(val as i32));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_push_int0(&mut self) -> ControlSignal {
        self.stack.push(VmValue::from_i32(0));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_push_int1(&mut self) -> ControlSignal {
        self.stack.push(VmValue::from_i32(1));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_push_int2(&mut self) -> ControlSignal {
        self.stack.push(VmValue::from_i32(2));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_push_int3(&mut self) -> ControlSignal {
        self.stack.push(VmValue::from_i32(3));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_push_int4(&mut self) -> ControlSignal {
        self.stack.push(VmValue::from_i32(4));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_push_int5(&mut self) -> ControlSignal {
        self.stack.push(VmValue::from_i32(5));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_push_int6(&mut self) -> ControlSignal {
        self.stack.push(VmValue::from_i32(6));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_push_int7(&mut self) -> ControlSignal {
        self.stack.push(VmValue::from_i32(7));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_push_int8(&mut self) -> ControlSignal {
        self.stack.push(VmValue::from_i32(8));
        ControlSignal::ContinueInstruction
    }
}
