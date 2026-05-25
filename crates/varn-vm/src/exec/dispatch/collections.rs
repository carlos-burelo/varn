use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::exec::dispatch::ControlSignal;
use crate::frame::VmClosure;
use crate::value::VmValue;

impl ExecCtx {
    #[inline(always)]
    pub(super) fn op_str_length(&mut self) -> VmResult<ControlSignal> {
        let val = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let nv = crate::exec::strings::str_length(val, &self.heap)?;
        self.stack.push(nv);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_str_concat(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let nv = crate::exec::strings::str_concat(a, b, &mut self.heap);
        self.stack.push(nv);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_str_slice(&mut self) -> VmResult<ControlSignal> {
        let len = self.stack.len();
        let end = self.stack[len - 1];
        let start = self.stack[len - 2];
        let s = self.stack[len - 3];
        unsafe { self.stack.set_len(len - 3) };
        let nv = crate::exec::strings::str_slice(s, start, end, &mut self.heap)?;
        self.stack.push(nv);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_array_length(&mut self) -> VmResult<ControlSignal> {
        let val = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let nv = crate::exec::collections::array_length(val, &self.heap)?;
        self.stack.push(nv);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_array_push(&mut self) -> VmResult<ControlSignal> {
        let val = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let arr = self.stack[self.stack.len() - 1];
        crate::exec::collections::array_push(arr, val, &mut self.heap)?;
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_array_pop(&mut self) -> VmResult<ControlSignal> {
        let arr = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let nv = crate::exec::collections::array_pop(arr, &mut self.heap)?;
        self.stack.push(nv);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_array_extend(&mut self) -> VmResult<ControlSignal> {
        let src = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let dst = self.stack[self.stack.len() - 1];
        crate::exec::collections::array_extend(dst, src, &self.heap)?;
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_object_keys(&mut self) -> VmResult<ControlSignal> {
        let obj = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let nv = crate::exec::collections::object_keys(obj, &mut self.heap)?;
        self.stack.push(nv);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_object_rest(&mut self) -> VmResult<ControlSignal> {
        let len = self.stack.len();
        let spread_nv = self.stack[len - 1];
        let target_nv = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };

        let spread = self.heap.extract(spread_nv);
        let target = self.heap.extract(target_nv);

        if let (varn_types::Value::Object(t), varn_types::Value::Object(s)) = (target, spread) {
            let src_props: Vec<(std::rc::Rc<str>, crate::value::VmValue)> = {
                let s_guard = s.borrow();
                s_guard
                    .inner
                    .shape
                    .property_names
                    .iter()
                    .map(|(name, &slot)| (name.clone(), s_guard.inner.values[slot]))
                    .collect()
            };

            let mut dst = t.borrow_mut();
            for (name, nv) in src_props {
                dst.inner.insert(name, nv);
            }
        }
        self.stack.push(target_nv);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_wrap_spread(&mut self) -> ControlSignal {
        let val = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let extracted = self.heap.extract(val);
        let spread = self
            .heap
            .intern(varn_types::Value::Spread(Box::new(extracted)));
        self.stack.push(spread);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_get_symbol(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        closure: &VmClosure,
    ) -> VmResult<ControlSignal> {
        let idx = code[*ip] as usize;
        *ip += 1;
        let symbol = match closure.proto.chunk.constants.get(idx) {
            Some(varn_types::PoolEntry::Literal(varn_types::Literal::Symbol(s))) => s.clone(),
            _ => return Err(crate::error::RuntimeError::new("OpGetSymbol: not a symbol")),
        };
        let val = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let nv = crate::exec::advanced::get_symbol_property(val, symbol, &mut self.heap)?;
        self.stack.push(nv);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_bind_method(&mut self) -> VmResult<ControlSignal> {
        let len = self.stack.len();
        let method = self.stack[len - 1];
        let receiver = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let v = crate::exec::advanced::bind_method(receiver, method, &mut self.heap)?;
        self.stack.push(v);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_make_enum_variant(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
    ) -> VmResult<ControlSignal> {
        let tag = code[*ip] as u8;
        *ip += 1;
        let idx = code[*ip] as usize;
        *ip += 1;
        let name = self.read_str_const_at(idx, frame_idx)?;
        let payload = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let nv = crate::exec::advanced::make_enum_variant(tag, &name, payload, &mut self.heap);
        self.stack.push(nv);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_get_enum_tag(&mut self) -> VmResult<ControlSignal> {
        let val = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let nv = crate::exec::advanced::get_enum_tag(val, &self.heap)?;
        self.stack.push(nv);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_invoke_runtime_static(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
    ) -> VmResult<ControlSignal> {
        let idx = code[*ip] as usize;
        *ip += 1;
        let flag = code[*ip] as u16;
        *ip += 1;
        let method_name = self.read_str_const_at(idx, frame_idx)?;
        let res = crate::exec::advanced::invoke_runtime_static(
            &method_name,
            &mut self.stack,
            &mut self.heap,
            flag,
        )?;
        self.stack.push(res);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub(super) fn op_yield(&mut self) -> ControlSignal {
        let val = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        self.vm_suspend = Some(crate::exec::VmSuspend::Yield {
            value: val,
            dest_reg: 0,
        });
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_spawn(&mut self) -> VmResult<ControlSignal> {
        let top = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let val = self.heap.extract(top);
        match val {
            varn_types::Value::Task(task) => {
                let fut = self.run_lazy_task_sync(task.as_ref());
                let nv = self.heap.intern(varn_types::Value::TaskHandle(fut));
                self.stack.push(nv);
            }
            varn_types::Value::TaskHandle(_) => self.stack.push(top),
            other => {
                return Err(crate::error::RuntimeError::new(format!(
                    "spawn expects Task, got {}",
                    other.type_name()
                )))
            }
        }
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_await(&mut self) -> ControlSignal {
        let top = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let val = self.heap.extract(top);
        match val {
            varn_types::Value::TaskHandle(task) => {
                self.vm_suspend = Some(crate::exec::VmSuspend::Task(task));
            }
            other => {
                let nv = self.heap.intern(other);
                self.stack.push(nv);
            }
        }
        ControlSignal::ContinueInstruction
    }
}
