use std::mem::MaybeUninit;

use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
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
                match &bm.target {
                    varn_types::value::BoundMethodTarget::Native { func, name, .. } => {
                        let f = *func;
                        self.record_call_native(f, Some(name));
                        let has_placeholder = base + arg_start < self.stack.len();
                        let result = if has_placeholder {
                            self.stack[base + arg_start] = receiver;
                            if arg_count <= 16 {
                                let mut buf: [MaybeUninit<VmValue>; 16] =
                                    unsafe { MaybeUninit::uninit().assume_init() };
                                let n = unsafe {
                                    copy_args_to_buf(
                                        &self.stack,
                                        base,
                                        arg_start,
                                        arg_count,
                                        &mut buf,
                                    )
                                };
                                let slice = unsafe {
                                    std::slice::from_raw_parts(buf.as_ptr().cast::<VmValue>(), n)
                                };
                                self.invoke_native(f, slice)
                            } else {
                                let varn_args: Vec<VmValue> = (0..arg_count)
                                    .map(|i| self.stack[base + arg_start + i])
                                    .collect();
                                self.invoke_native(f, &varn_args)
                            }
                        } else {
                            let mut varn_args = Vec::with_capacity(arg_count + 1);
                            varn_args.push(receiver);
                            for i in 0..arg_count {
                                varn_args.push(self.stack[base + arg_start + i]);
                            }
                            self.invoke_native(f, &varn_args)
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
                            .downcast_ref::<crate::closure::VmClosurePayload>()
                        {
                            let nc = &nc_w.0;
                            let arity = nc.proto.arity;
                            if !nc.proto.is_generator
                                && !nc.proto.is_async
                                && (arg_count == arity || arg_count == arity - 1)
                            {
                                let nc = nc.clone();
                                let owner = owner_class.clone();
                                self.stack.push(VmValue::null());

                                if arg_count == arity - 1 {
                                    self.stack.push(receiver);
                                    for i in 0..arg_count {
                                        let v = self.stack[base + arg_start + i];
                                        self.stack.push(v);
                                    }
                                } else {
                                    self.stack.push(receiver);
                                    for i in 1..arg_count {
                                        let v = self.stack[base + arg_start + i];
                                        self.stack.push(v);
                                    }
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
                                let mut frame = crate::frame::CallFrame::new_owned(nc, new_base);
                                frame.return_reg = Some(dest as u16);
                                frame.current_class = owner;
                                self.record_call_vm_fast();
                                self.frames.push(frame);
                                return Ok(true);
                            }
                        }
                    }
                }

                let has_placeholder = match &bm.target {
                    varn_types::value::BoundMethodTarget::Vm { closure, .. } => {
                        if let Some(nc_w) = closure
                            .as_any()
                            .downcast_ref::<crate::closure::VmClosurePayload>()
                        {
                            arg_count == nc_w.0.proto.arity
                        } else {
                            false
                        }
                    }
                    varn_types::value::BoundMethodTarget::Native { .. } => {
                        base + arg_start < self.stack.len()
                    }
                };
                if has_placeholder {
                    self.stack[base + arg_start] = receiver;
                }
            } else {
                match self.heap.get(callee.as_heap_idx()) {
                    Some(crate::heap::HeapObj::NativeFn(f, name)) => {
                        let f = *f;
                        let name_str = *name;
                        self.record_call_native(f, Some(name_str));
                        let result = if arg_count <= 16 {
                            let mut buf: [MaybeUninit<VmValue>; 16] =
                                unsafe { MaybeUninit::uninit().assume_init() };
                            let n = unsafe {
                                copy_args_to_buf(&self.stack, base, arg_start, arg_count, &mut buf)
                            };

                            let slice = if n > 1 {
                                unsafe {
                                    std::slice::from_raw_parts(
                                        buf.as_ptr().cast::<VmValue>().add(1),
                                        n - 1,
                                    )
                                }
                            } else {
                                &[]
                            };
                            self.invoke_native(f, slice)
                        } else {
                            let varn_args: Vec<VmValue> = (0..arg_count)
                                .map(|i| self.stack[base + arg_start + i])
                                .collect();
                            let slice = if arg_count > 1 { &varn_args[1..] } else { &[] };
                            self.invoke_native(f, slice)
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
                                let _fn_name =
                                    nc.proto.name.as_deref().unwrap_or("<anon>").to_owned();
                                let _is_jit = nc.jit_fn().is_some();
                                let new_base = self.stack.len();
                                if self.frames.len() >= 10000 {
                                    return Err(crate::error::RuntimeError::new(
                                        "stack overflow: call depth exceeded 10000",
                                    ));
                                }
                                let required = new_base + nc.proto.register_count as usize;
                                if self.stack.len() < required {
                                    self.stack.resize(required, VmValue::null());
                                }
                                if arg_count > 0 {
                                    unsafe {
                                        let src = self.stack.as_ptr().add(base + arg_start);
                                        let dst = self.stack.as_mut_ptr().add(new_base);
                                        std::ptr::copy_nonoverlapping(src, dst, arg_count);
                                    }
                                }
                                let mut frame = crate::frame::CallFrame::new_owned(nc, new_base);
                                frame.return_reg = Some(dest as u16);
                                self.record_call_vm_fast();
                                self.frames.push(frame);
                                return Ok(true);
                            } else if nc.proto.has_rest && arg_count <= arity {
                                let nc = nc.clone();
                                let fn_name2 =
                                    nc.proto.name.as_deref().unwrap_or("<anon>").to_owned();
                                let is_jit2 = nc.jit_fn().is_some();
                                self.record_hotspot_fn(&fn_name2, is_jit2);
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
                                let mut frame = crate::frame::CallFrame::new_owned(nc, new_base);
                                frame.return_reg = Some(dest as u16);
                                self.record_call_vm_fast();
                                self.frames.push(frame);
                                return Ok(true);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        for i in 0..arg_count {
            let v = self.stack[base + arg_start + i];
            self.push(v);
        }

        let prepared = self.prepare_call(callee, arg_count)?;
        self.dispatch_prepared_call(prepared)?;

        if self.frames.len() > frame_idx + 1 {
            self.frames.last_mut().unwrap().return_reg = Some(dest as u16);
            let last = self.frames.last().unwrap();
            let req = last.base + last.closure().proto.register_count as usize;
            if self.stack.len() < req {
                self.stack.resize(req, VmValue::null());
            }
            return Ok(true);
        }

        let result = self.stack.pop().unwrap_or(VmValue::null());
        let caller_frame = &self.frames[frame_idx];
        let required = base + caller_frame.closure().proto.register_count as usize;
        if self.stack.len() < required {
            self.stack.resize(required, VmValue::null());
        }
        self.stack[base + dest] = result;
        Ok(false)
    }

    pub(crate) fn exec_call_self(
        &mut self,
        base: usize,
        arg_start: usize,
        arg_count: usize,
        dest: usize,
        frame_idx: usize,
    ) -> VmResult<bool> {
        let parent_frame = &self.frames[frame_idx];
        let closure_ptr = parent_frame.closure_ptr;
        let closure_ref = unsafe { &*closure_ptr };

        let callee = if parent_frame.base > 0 {
            self.stack[parent_frame.base - 1]
        } else {
            VmValue::null()
        };

        if !closure_ref.proto.is_generator && !closure_ref.proto.is_async {
            let arity = closure_ref.proto.arity;

            if !closure_ref.proto.has_rest && arg_count <= arity {
                let _fn_name = closure_ref
                    .proto
                    .name
                    .as_deref()
                    .unwrap_or("<anon>")
                    .to_owned();
                let _is_jit = closure_ref.jit_fn().is_some();
                let new_base = self.stack.len();
                if self.frames.len() >= 10000 {
                    return Err(crate::error::RuntimeError::new(
                        "stack overflow: call depth exceeded 10000",
                    ));
                }
                let required = new_base + closure_ref.proto.register_count as usize;
                if self.stack.len() < required {
                    self.stack.resize(required, VmValue::null());
                }
                if arg_count > 0 {
                    unsafe {
                        let src = self.stack.as_ptr().add(base + arg_start);
                        let dst = self.stack.as_mut_ptr().add(new_base);
                        std::ptr::copy_nonoverlapping(src, dst, arg_count);
                    }
                }
                let mut frame = crate::frame::CallFrame::new(closure_ref, new_base);
                frame._owned_closure = self.frames[frame_idx]._owned_closure.clone();
                frame.return_reg = Some(dest as u16);
                frame.caller_base = Some(base);
                self.record_call_vm_fast();
                self.frames.push(frame);
                return Ok(true);
            } else if closure_ref.proto.has_rest && arg_count <= arity {
                let fn_name2 = closure_ref
                    .proto
                    .name
                    .as_deref()
                    .unwrap_or("<anon>")
                    .to_owned();
                let is_jit2 = closure_ref.jit_fn().is_some();
                self.record_hotspot_fn(&fn_name2, is_jit2);
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
                let rest_nv = VmValue::from_heap_idx(
                    self.heap
                        .alloc(crate::heap::HeapObj::Array(VmArray::new(rest_items))),
                );
                self.stack.push(rest_nv);
                if self.frames.len() >= 10000 {
                    return Err(crate::error::RuntimeError::new(
                        "stack overflow: call depth exceeded 10000",
                    ));
                }
                let new_base = self.stack.len() - arity;
                let required = new_base + closure_ref.proto.register_count as usize;
                if self.stack.len() < required {
                    self.stack.resize(required, VmValue::null());
                }
                let mut frame = crate::frame::CallFrame::new(closure_ref, new_base);
                frame._owned_closure = self.frames[frame_idx]._owned_closure.clone();
                frame.return_reg = Some(dest as u16);
                self.record_call_vm_fast();
                self.frames.push(frame);
                return Ok(true);
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
            let req = last.base + last.closure().proto.register_count as usize;
            if self.stack.len() < req {
                self.stack.resize(req, VmValue::null());
            }
            return Ok(true);
        }
        let result = self.stack.pop().unwrap_or(VmValue::null());
        let caller_frame = &self.frames[frame_idx];
        let required = base + caller_frame.closure().proto.register_count as usize;
        if self.stack.len() < required {
            self.stack.resize(required, VmValue::null());
        }
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
        let mut expanded = Vec::new();
        for i in 0..arg_count {
            let nv = self.stack[base + arg_start + i];
            match self.heap.extract(nv) {
                Value::Spread(inner) => match *inner {
                    Value::Array(arr) => {
                        for v in arr.borrow().iter().cloned() {
                            expanded.push(self.heap.intern(v));
                        }
                    }
                    other => expanded.push(self.heap.intern(other)),
                },
                Value::Array(arr) => {
                    for v in arr.borrow().iter().cloned() {
                        expanded.push(self.heap.intern(v));
                    }
                }
                other => expanded.push(self.heap.intern(other)),
            }
        }
        let flat_count = expanded.len();
        self.push(callee);
        for nv in expanded {
            self.push(nv);
        }
        let prepared = self.prepare_call(callee, flat_count)?;
        self.dispatch_prepared_call(prepared)?;

        if self.frames.len() > frame_idx + 1 {
            self.frames.last_mut().unwrap().return_reg = Some(dest as u16);
            let last = self.frames.last().unwrap();
            let req = last.base + last.closure().proto.register_count as usize;
            if self.stack.len() < req {
                self.stack.resize(req, VmValue::null());
            }
            return Ok(true);
        }

        let result = self.stack.pop().unwrap_or(VmValue::null());
        let caller_frame = &self.frames[frame_idx];
        let required = base + caller_frame.closure().proto.register_count as usize;
        if self.stack.len() < required {
            self.stack.resize(required, VmValue::null());
        }
        self.stack[base + dest] = result;
        Ok(false)
    }
}
