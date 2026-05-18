use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::exec::dispatch::ControlSignal;
use crate::frame::VmClosure;
use crate::value::VmValue;

impl ExecCtx {
    #[inline(always)]
    pub(super) fn op_get_local(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        base: usize,
    ) -> ControlSignal {
        let slot = code[*ip] as usize;
        *ip += 1;
        let idx = base + slot;
        let v = self.stack[idx];
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_set_local(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        base: usize,
    ) -> ControlSignal {
        let slot = code[*ip] as usize;
        *ip += 1;
        let idx = base + slot;
        let v = self.stack[self.stack.len() - 1];
        self.stack[idx] = v;
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_set_local_drop(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        base: usize,
    ) -> ControlSignal {
        let slot = code[*ip] as usize;
        *ip += 1;
        let idx = base + slot;
        let v = {
            let new_len = self.stack.len() - 1;
            let v = self.stack[new_len];
            unsafe { self.stack.set_len(new_len) };
            v
        };
        self.stack[idx] = v;
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_global_index(&mut self, code: &[u16], ip: &mut usize) -> ControlSignal {
        let idx = code[*ip] as usize;
        *ip += 1;
        let v = if idx < self.globals.values.len() {
            self.globals.values[idx]
        } else {
            VmValue::null()
        };
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_set_global_index(&mut self, code: &[u16], ip: &mut usize) -> ControlSignal {
        let idx = code[*ip] as usize;
        *ip += 1;
        let v = self.stack[self.stack.len() - 1];
        self.globals.set_by_index(idx, v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_local0(&mut self, base: usize) -> ControlSignal {
        let v = self.stack[base];
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_local1(&mut self, base: usize) -> ControlSignal {
        let v = self.stack[base + 1];
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_local2(&mut self, base: usize) -> ControlSignal {
        let v = self.stack[base + 2];
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_local3(&mut self, base: usize) -> ControlSignal {
        let v = self.stack[base + 3];
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_local4(&mut self, base: usize) -> ControlSignal {
        let v = self.stack[base + 4];
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_local5(&mut self, base: usize) -> ControlSignal {
        let v = self.stack[base + 5];
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_local6(&mut self, base: usize) -> ControlSignal {
        let v = self.stack[base + 6];
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_local7(&mut self, base: usize) -> ControlSignal {
        let v = self.stack[base + 7];
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_local8(&mut self, base: usize) -> ControlSignal {
        let v = self.stack[base + 8];
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_global0(&mut self) -> ControlSignal {
        let v = self.globals.get_by_index_unchecked(0);
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_global1(&mut self) -> ControlSignal {
        let v = self.globals.get_by_index_unchecked(1);
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_global2(&mut self) -> ControlSignal {
        let v = self.globals.get_by_index_unchecked(2);
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_global3(&mut self) -> ControlSignal {
        let v = self.globals.get_by_index_unchecked(3);
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_global4(&mut self) -> ControlSignal {
        let v = self.globals.get_by_index_unchecked(4);
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_global5(&mut self) -> ControlSignal {
        let v = self.globals.get_by_index_unchecked(5);
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_global6(&mut self) -> ControlSignal {
        let v = self.globals.get_by_index_unchecked(6);
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_global7(&mut self) -> ControlSignal {
        let v = self.globals.get_by_index_unchecked(7);
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_global8(&mut self) -> ControlSignal {
        let v = self.globals.get_by_index_unchecked(8);
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_global(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
    ) -> VmResult<ControlSignal> {
        let idx = code[*ip] as usize;
        *ip += 1;
        let name = self.read_str_const_at(idx, frame_idx)?;
        let v = crate::exec::vars::get_global_by_name(&name, &self.globals);
        self.stack.push(v);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_set_global(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
    ) -> VmResult<ControlSignal> {
        let idx = code[*ip] as usize;
        *ip += 1;
        let name = self.read_str_const_at(idx, frame_idx)?;
        let v = *self.stack.last().expect("OpSetGlobal: stack empty");
        crate::exec::vars::set_global_by_name(&name, v, &mut self.globals);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_define_global(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
    ) -> VmResult<ControlSignal> {
        let idx = code[*ip] as usize;
        *ip += 1;
        let name = self.read_str_const_at(idx, frame_idx)?;
        let v = self.stack.pop().expect("OpDefineGlobal: stack empty");
        self.globals.define(&name, v);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_get_upvalue(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        closure: &VmClosure,
    ) -> ControlSignal {
        let idx = code[*ip] as usize;
        *ip += 1;
        let v = closure.upvalues[idx].read(&self.stack);
        self.stack.push(v);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_set_upvalue(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        closure: &VmClosure,
    ) -> ControlSignal {
        let idx = code[*ip] as usize;
        *ip += 1;
        let v = *self.stack.last().expect("OpSetUpvalue: stack empty");
        closure.upvalues[idx].write(v, &mut self.stack);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_close_upvalue(&mut self) -> ControlSignal {
        let idx = self.stack.len() - 1;
        self.close_upvalues_above(idx);
        self.stack.pop().expect("OpCloseUpvalue: stack empty");
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_close_upvalue_at(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        base: usize,
    ) -> ControlSignal {
        let idx = code[*ip] as usize;
        *ip += 1;
        self.close_upvalues_above(base + idx);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_define_global_index(&mut self, code: &[u16], ip: &mut usize) -> ControlSignal {
        let idx = code[*ip] as usize;
        *ip += 1;
        let v = self.stack.pop().expect("OpDefineGlobalIndex: stack empty");
        self.globals.set_by_index(idx, v);
        ControlSignal::ContinueInstruction
    }
}
