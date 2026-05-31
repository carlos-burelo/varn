use std::mem::MaybeUninit;

use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::frame::VmClosure;
use crate::value::VmValue;
use varn_types::{Value, VmArray};

#[inline(always)]
unsafe fn copy_args_to_buf(
    stack: &[VmValue],
    base: usize,
    arg_start: usize,
    arg_count: usize,
    buf: &mut [MaybeUninit<VmValue>; 16],
) -> usize {
    let n = arg_count.min(16);
    if n > 0 {
        let src_ptr = stack.as_ptr().add(base + arg_start);
        let dest_ptr = buf.as_mut_ptr() as *mut VmValue;
        std::ptr::copy_nonoverlapping(src_ptr, dest_ptr, n);
    }
    n
}

impl ExecCtx {
    pub(crate) fn exec_call_reg(
        &mut self,
        callee: VmValue,
        base: usize,
        arg_start: usize,
        arg_count: usize,
        dest: usize,
        frame_idx: usize,
    ) -> VmResult<bool> {
        if callee.is_heap() {
            let mut bound_method = None;
            if let Some(crate::heap::HeapObj::BoundMethod(bm)) = self.heap.get(callee.as_heap_idx())
            {
                bound_method = Some((**bm).clone());
            }

            if let Some(bm) = bound_method {
                let receiver = self.heap.intern(bm.receiver.clone());
                self.stack[base + arg_start] = receiver;
                match &bm.target {
                    varn_types::value::BoundMethodTarget::Native { func, .. } => {
                        let f = *func;
                        self.record_call_native();
                        let result = if arg_count <= 16 {
                            let mut buf: [MaybeUninit<VmValue>; 16] =
                                unsafe { MaybeUninit::uninit().assume_init() };
                            let n = unsafe {
                                copy_args_to_buf(&self.stack, base, arg_start, arg_count, &mut buf)
                            };
                            let slice = unsafe {
                                std::slice::from_raw_parts(buf.as_ptr().cast::<VmValue>(), n)
                            };
                            (f)(self as &mut dyn varn_types::NativeCtx, slice)
                        } else {
                            let varn_args: Vec<VmValue> = (0..arg_count)
                                .map(|i| self.stack[base + arg_start + i])
                                .collect();
                            (f)(self as &mut dyn varn_types::NativeCtx, &varn_args)
                        }
                        .map_err(|e| crate::error::RuntimeError::new(e))?;
                        self.stack[base + dest] = result;
                        return Ok(false);
                    }
                    varn_types::value::BoundMethodTarget::Vm {
                        closure: method_closure,
                        owner_class,
                    } => {
                        if let Some(nc_w) = method_closure
                            .as_any()
                            .downcast_ref::<crate::frame::VmClosurePayload>()
                        {
                            let nc = &nc_w.0;
                            if !nc.proto.is_generator
                                && !nc.proto.is_async
                                && arg_count <= nc.proto.arity
                            {
                                let nc = nc.clone();
                                let owner = owner_class.clone();
                                let arity = nc.proto.arity;
                                self.stack.push(VmValue::null());
                                if !nc.proto.has_rest {
                                    for i in 0..arg_count {
                                        let v = self.stack[base + arg_start + i];
                                        self.stack.push(v);
                                    }
                                    for _ in arg_count..arity {
                                        self.stack.push(VmValue::null());
                                    }
                                } else {
                                    let rest_idx = arity.saturating_sub(1);
                                    let regular_count = arg_count.min(rest_idx);
                                    for i in 0..regular_count {
                                        let v = self.stack[base + arg_start + i];
                                        self.stack.push(v);
                                    }
                                    for _ in regular_count..rest_idx {
                                        self.stack.push(VmValue::null());
                                    }
                                    let rest_items: Vec<VmValue> = if arg_count > rest_idx {
                                        (rest_idx..arg_count)
                                            .map(|i| self.stack[base + arg_start + i])
                                            .collect()
                                    } else {
                                        vec![]
                                    };
                                    let rest_nv = VmValue::from_heap_idx(self.heap.alloc(
                                        crate::heap::HeapObj::Array(VmArray::new(rest_items)),
                                    ));
                                    self.stack.push(rest_nv);
                                }
                                if self.frames.len() >= 10000 {
                                    return Err(crate::error::RuntimeError::new(
                                        "stack overflow: call depth exceeded 10000",
                                    ));
                                }
                                let new_base = self.stack.len() - arity;
                                let required = new_base + nc.proto.register_count as usize;
                                if self.stack.len() < required {
                                    self.stack.resize(required, VmValue::null());
                                }
                                let mut frame = crate::frame::CallFrame::new(nc, new_base);
                                frame.return_reg = Some(dest as u16);
                                frame.current_class = owner;
                                self.record_call_vm_fast();
                                self.record_frame_push();
                                self.frames.push(frame);
                                return Ok(true);
                            }
                        }
                    }
                }
            } else {
                match self.heap.get(callee.as_heap_idx()) {
                    Some(crate::heap::HeapObj::NativeFn(_name, f)) => {
                        let f = *f;
                        self.record_call_native();
                        let result = if arg_count <= 16 {
                            let mut buf: [MaybeUninit<VmValue>; 16] =
                                unsafe { MaybeUninit::uninit().assume_init() };
                            let n = unsafe {
                                copy_args_to_buf(&self.stack, base, arg_start, arg_count, &mut buf)
                            };
                            // skip slot 0 (callee/receiver), pass actual args
                            let slice = if n > 0 {
                                unsafe {
                                    std::slice::from_raw_parts(
                                        buf.as_ptr().cast::<VmValue>().add(1),
                                        n - 1,
                                    )
                                }
                            } else {
                                &[]
                            };
                            (f)(self as &mut dyn varn_types::NativeCtx, slice)
                        } else {
                            let varn_args: Vec<VmValue> = (0..arg_count)
                                .map(|i| self.stack[base + arg_start + i])
                                .collect();
                            let slice = if arg_count > 0 {
                                &varn_args[1..]
                            } else {
                                &varn_args[..]
                            };
                            (f)(self as &mut dyn varn_types::NativeCtx, slice)
                        }
                        .map_err(|e| crate::error::RuntimeError::new(e))?;
                        self.stack[base + dest] = result;
                        return Ok(false);
                    }
                    Some(crate::heap::HeapObj::VmClosure(nc)) => {
                        if !nc.proto.is_generator && !nc.proto.is_async {
                            let arity = nc.proto.arity;

                            if !nc.proto.has_rest && arg_count <= arity {
                                let nc = nc.clone();
                                self.stack.push(callee);
                                for i in 0..arg_count {
                                    let v = self.stack[base + arg_start + i];
                                    self.stack.push(v);
                                }

                                for _ in arg_count..arity {
                                    self.stack.push(VmValue::null());
                                }
                                if self.frames.len() >= 10000 {
                                    return Err(crate::error::RuntimeError::new(
                                        "stack overflow: call depth exceeded 10000",
                                    ));
                                }
                                let new_base = self.stack.len() - arity;
                                let required = new_base + nc.proto.register_count as usize;
                                if self.stack.len() < required {
                                    self.stack.resize(required, VmValue::null());
                                }
                                let mut frame = crate::frame::CallFrame::new(nc, new_base);
                                frame.return_reg = Some(dest as u16);
                                self.record_call_vm_fast();
                                self.record_frame_push();
                                self.frames.push(frame);
                                return Ok(true);
                            } else if nc.proto.has_rest && arg_count <= arity {
                                let nc = nc.clone();
                                let rest_idx = arity.saturating_sub(1);
                                self.stack.push(callee);
                                let regular_count = arg_count.min(rest_idx);
                                for i in 0..regular_count {
                                    let v = self.stack[base + arg_start + i];
                                    self.stack.push(v);
                                }
                                for _ in regular_count..rest_idx {
                                    self.stack.push(VmValue::null());
                                }
                                let rest_items: Vec<VmValue> = if arg_count > rest_idx {
                                    (rest_idx..arg_count)
                                        .map(|i| self.stack[base + arg_start + i])
                                        .collect()
                                } else {
                                    vec![]
                                };
                                let rest_nv =
                                    VmValue::from_heap_idx(self.heap.alloc(
                                        crate::heap::HeapObj::Array(VmArray::new(rest_items)),
                                    ));
                                self.stack.push(rest_nv);
                                if self.frames.len() >= 10000 {
                                    return Err(crate::error::RuntimeError::new(
                                        "stack overflow: call depth exceeded 10000",
                                    ));
                                }
                                let new_base = self.stack.len() - arity;
                                let required = new_base + nc.proto.register_count as usize;
                                if self.stack.len() < required {
                                    self.stack.resize(required, VmValue::null());
                                }
                                let mut frame = crate::frame::CallFrame::new(nc, new_base);
                                frame.return_reg = Some(dest as u16);
                                self.record_call_vm_fast();
                                self.record_frame_push();
                                self.frames.push(frame);
                                return Ok(true);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        self.push(callee);
        for i in 0..arg_count {
            let v = self.stack[base + arg_start + i];
            self.push(v);
        }

        let prepared = self.prepare_call(callee, arg_count)?;
        self.dispatch_prepared_call(prepared)?;

        if self.frames.len() > frame_idx + 1 {
            self.frames.last_mut().unwrap().return_reg = Some(dest as u16);
            let last = self.frames.last().unwrap();
            let req = last.base + last.closure.proto.register_count as usize;
            if self.stack.len() < req {
                self.stack.resize(req, VmValue::null());
            }
            return Ok(true);
        }

        let result = self.stack.pop().unwrap_or(VmValue::null());
        self.stack[base + dest] = result;
        Ok(false)
    }

    pub(crate) fn exec_call_spread_reg(
        &mut self,
        callee: VmValue,
        base: usize,
        arg_start: usize,
        arg_count: usize,
        dest: usize,
        frame_idx: usize,
    ) -> VmResult<bool> {
        self.push(callee);
        let mut expanded = Vec::new();
        for i in 0..arg_count {
            let nv = self.stack[base + arg_start + i];
            match self.heap.extract(nv) {
                Value::Spread(inner) => match *inner {
                    Value::Array(arr) => {
                        for v in arr.0.borrow().iter().cloned() {
                            expanded.push(self.heap.intern(v));
                        }
                    }
                    other => expanded.push(self.heap.intern(other)),
                },
                other => expanded.push(self.heap.intern(other)),
            }
        }
        let flat_count = expanded.len();
        for nv in expanded {
            self.push(nv);
        }
        let prepared = self.prepare_call(callee, flat_count)?;
        self.dispatch_prepared_call(prepared)?;

        if self.frames.len() > frame_idx + 1 {
            self.frames.last_mut().unwrap().return_reg = Some(dest as u16);
            let last = self.frames.last().unwrap();
            let req = last.base + last.closure.proto.register_count as usize;
            if self.stack.len() < req {
                self.stack.resize(req, VmValue::null());
            }
            return Ok(true);
        }

        let result = self.stack.pop().unwrap_or(VmValue::null());
        self.stack[base + dest] = result;
        Ok(false)
    }
}
