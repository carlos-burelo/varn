use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::frame::{VmClosure, VmClosurePayload};
use crate::value::VmValue;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use varn_types::{Value, VmArray};

impl ExecCtx {
    pub(crate) fn exec_call_method_reg(
        &mut self,
        this_val: VmValue,
        base: usize,
        name_idx: usize,
        cs: usize,
        arg_start: usize,
        arg_count: usize,
        dest: usize,
        frame_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<bool> {
        let name_nv = closure.constants[name_idx];
        let name = self
            .heap
            .str_val(name_nv)
            .ok_or_else(|| RuntimeError::new("CallMethod: non-string name"))?;

        let is_megamorphic = closure
            .feedback
            .borrow()
            .sites
            .get(cs)
            .map(|s| s.megamorphic)
            .unwrap_or(false);

        let receiver_class = crate::exec::props::get_class(this_val, &self.heap);
        let cache_len = closure.ic_cache_len();
        if receiver_class.is_some() && cs < cache_len && !is_megamorphic {
            type NativeFnPtr =
                fn(&mut dyn varn_types::NativeCtx, &[VmValue]) -> Result<VmValue, String>;

            let ic_native: Option<(NativeFnPtr, VmValue)>;
            let ic_vm: Option<(Rc<VmClosure>, Option<Rc<varn_types::value::ClassObj>>)>;
            {
                let ic = closure.ic_cache.borrow();
                let poly = &ic[cs];
                let mut found_nat: Option<(NativeFnPtr, VmValue)> = None;
                let mut found_vm: Option<(Rc<VmClosure>, Option<Rc<varn_types::value::ClassObj>>)> =
                    None;
                'ic: for entry in &poly.entries {
                    if entry.id == 0 {
                        continue;
                    }
                    if entry.is_class == 6 {
                        if let Some(ref cls) = receiver_class {
                            if cls.id == entry.id
                                && entry.vtable_ver
                                    == (cls.vtable_version.load(Ordering::Relaxed) & 0xFF) as u8
                            {
                                let slot = entry.slot as usize;
                                let vtable = cls.vtable.borrow();
                                if let Some(Value::NativeFn(b)) = vtable.get(slot) {
                                    let f = b.0;
                                    found_nat = Some((f, this_val));
                                    break 'ic;
                                }
                            }
                        }
                    } else if entry.is_class == 7 {
                        if let Some(ref cls) = receiver_class {
                            let cls_ver = (cls.vtable_version.load(Ordering::Relaxed) & 0xFF) as u8;
                            if cls.id == entry.id && cls_ver == entry.vtable_ver {
                                let slot = entry.slot as usize;
                                let method_val = cls.vtable.borrow().get(slot).cloned();
                                if let Some(Value::VmValue(payload)) = method_val {
                                    if let Some(nc_w) =
                                        payload.as_any().downcast_ref::<VmClosurePayload>()
                                    {
                                        let nc = nc_w.0.clone();
                                        if !nc.proto.is_generator
                                            && !nc.proto.is_async
                                            && arg_count <= nc.proto.arity as usize
                                        {
                                            found_vm = Some((nc, Some(cls.clone())));
                                            break 'ic;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                ic_native = found_nat;
                ic_vm = found_vm;
            }

            if let Some((f, receiver)) = ic_native {
                self.record_ic_hit_callmethod();
                self.record_call_native();
                let result = self.call_native_with_receiver(f, receiver, base, arg_start, arg_count)?;
                self.stack[base + dest] = result;
                return Ok(false);
            }

            if let Some((nc, owner_class)) = ic_vm {
                self.record_ic_hit_callmethod();
                self.record_call_vm_fast();
                self.stack.push(VmValue::null());
                if !nc.proto.has_rest {
                    self.stack.push(this_val);
                    for i in 0..arg_count {
                        let v = self.stack[base + arg_start + i];
                        self.stack.push(v);
                    }
                } else {
                    // Inline rest-arg bundling: this_val is arg 0, rest-param is last.
                    let arity = nc.proto.arity;
                    let rest_idx = arity.saturating_sub(1);
                    self.stack.push(this_val);
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
                }
                let full_arg_count = nc.proto.arity;
                if self.frames.len() >= 10000 {
                    return Err(crate::error::RuntimeError::new(
                        "stack overflow: call depth exceeded 10000",
                    ));
                }
                let final_base = self.stack.len() - full_arg_count;
                let required = final_base + nc.proto.register_count as usize;
                if self.stack.len() < required {
                    self.stack.resize(required, VmValue::null());
                }
                let mut frame = crate::frame::CallFrame::new(nc, final_base);
                frame.return_reg = Some(dest as u16);
                frame.current_class = owner_class;
                self.record_frame_push();
                self.frames.push(frame);
                return Ok(true);
            }

            self.record_ic_miss_callmethod();
        }

        let method_nv = crate::exec::props::get_property(this_val, &name, &mut self.heap)?;

        if let Some(ref cls) = receiver_class {
            if cs < cache_len && method_nv.is_heap() && !is_megamorphic {
                if let Some(crate::heap::HeapObj::BoundMethod(bm)) =
                    self.heap.get(method_nv.as_heap_idx())
                {
                    match &bm.target {
                        varn_types::value::BoundMethodTarget::Native { .. } => {
                            if let Some(&slot) = cls.method_map.borrow().get(name.as_ref()) {
                                let entry = varn_types::chunk::CacheEntry {
                                    id: cls.id,
                                    slot: slot as u16,
                                    is_class: 6,
                                    vtable_ver: (cls.vtable_version.load(Ordering::Relaxed) & 0xFF)
                                        as u8,
                                };
                                closure.ic_cache.borrow_mut()[cs].find_or_insert(entry);
                                closure.feedback.borrow_mut().observe(cs, cls.id);
                            }
                        }
                        varn_types::value::BoundMethodTarget::Vm {
                            closure: method_closure,
                            ..
                        } => {
                            if let Some(nc_w) =
                                method_closure.as_any().downcast_ref::<VmClosurePayload>()
                            {
                                let nc = &nc_w.0;
                                if !nc.proto.is_generator && !nc.proto.is_async {
                                    if let Some(&slot) = cls.method_map.borrow().get(name.as_ref())
                                    {
                                        let entry = varn_types::chunk::CacheEntry {
                                            id: cls.id,
                                            slot: slot as u16,
                                            is_class: 7,
                                            vtable_ver: (cls.vtable_version.load(Ordering::Relaxed)
                                                & 0xFF)
                                                as u8,
                                        };
                                        closure.ic_cache.borrow_mut()[cs].find_or_insert(entry);
                                        closure.feedback.borrow_mut().observe(cs, cls.id);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let native_call: Option<(
            fn(&mut dyn varn_types::NativeCtx, &[VmValue]) -> Result<VmValue, String>,
            VmValue,
        )> = if method_nv.is_heap() {
            match self.heap.get(method_nv.as_heap_idx()) {
                Some(crate::heap::HeapObj::BoundMethod(bm)) => {
                    if let varn_types::value::BoundMethodTarget::Native { func: f, .. } = &bm.target
                    {
                        Some((*f, this_val))
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            None
        };
        if let Some((f, receiver)) = native_call {
            self.record_call_native();
            let result = self.call_native_with_receiver(f, receiver, base, arg_start, arg_count)?;
            self.stack[base + dest] = result;
            return Ok(false);
        }

        let is_bound = method_nv.is_heap()
            && matches!(
                self.heap.get(method_nv.as_heap_idx()),
                Some(crate::heap::HeapObj::BoundMethod(_))
            );
        let is_static = this_val.is_heap()
            && matches!(
                self.heap.get(this_val.as_heap_idx()),
                Some(crate::heap::HeapObj::Class(_))
            );
        let is_enum_variant = method_nv.is_heap()
            && matches!(
                self.heap.get(method_nv.as_heap_idx()),
                Some(crate::heap::HeapObj::EnumVariant(_))
            );
        let is_plain_closure_no_this = method_nv.is_heap() && {
            if let Some(crate::heap::HeapObj::VmClosure(nc)) =
                self.heap.get(method_nv.as_heap_idx())
            {
                !nc.proto.has_this
            } else {
                false
            }
        };
        let is_namespace_native = method_nv.is_heap()
            && this_val.is_heap()
            && matches!(
                self.heap.get(method_nv.as_heap_idx()),
                Some(crate::heap::HeapObj::NativeFn(..))
            )
            && matches!(
                self.heap.get(this_val.as_heap_idx()),
                Some(crate::heap::HeapObj::Object(_))
            );
        let is_plain_native = method_nv.is_heap()
            && matches!(
                self.heap.get(method_nv.as_heap_idx()),
                Some(crate::heap::HeapObj::NativeFn(..))
            );

        self.push(method_nv);

        let skip_this = is_bound
            || is_static
            || is_enum_variant
            || is_plain_closure_no_this
            || is_namespace_native
            || is_plain_native;
        if !skip_this {
            self.push(this_val);
        } else {
            self.push(VmValue::null());
        }
        for i in 0..arg_count {
            let v = self.stack[base + arg_start + i];
            self.push(v);
        }
        let effective_count = arg_count + 1;

        let prepared = self.prepare_call(method_nv, effective_count)?;
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

    /// Helper: call a native function with a receiver as first arg, followed by
    /// `arg_count` args from `self.stack[base + arg_start..]`.
    #[inline(always)]
    fn call_native_with_receiver(
        &mut self,
        f: fn(&mut dyn varn_types::NativeCtx, &[VmValue]) -> Result<VmValue, String>,
        receiver: VmValue,
        base: usize,
        arg_start: usize,
        arg_count: usize,
    ) -> VmResult<VmValue> {
        let result = if arg_count + 1 <= 16 {
            let mut buf = [VmValue::null(); 17];
            buf[0] = receiver;
            for i in 0..arg_count {
                buf[i + 1] = self.stack[base + arg_start + i];
            }
            (f)(self as &mut dyn varn_types::NativeCtx, &buf[..arg_count + 1])
        } else {
            let mut args = Vec::with_capacity(arg_count + 1);
            args.push(receiver);
            for i in 0..arg_count {
                args.push(self.stack[base + arg_start + i]);
            }
            (f)(self as &mut dyn varn_types::NativeCtx, &args)
        }
        .map_err(|e| RuntimeError::new(e))?;
        Ok(result)
    }
}
