use crate::closure::{VmClosure, VmClosurePayload};
use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use varn_types::chunk::ICKind;
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

        // `Enum.Variant(args)` reaches the VM as a method call on the enum's own
        // class object. It has to be answered before anything else: the inline
        // cache below can never help, because a variant is not in the class's
        // `method_map`, and the generic path underneath allocates the variant
        // template into the heap on every construction just to hand it to
        // `prepare_call`. See `ExecCtx::construct_enum_variant`.
        if let Some(template) = self.enum_variant_template(this_val, name.as_ref()) {
            if let Some(built) = self.construct_enum_variant(&template, base, arg_start, arg_count)
            {
                self.stack[base + dest] = built;
                return Ok(false);
            }
        }

        // Direct fast-path for high-frequency intrinsic Array methods (push, pop).
        // Eliminates method table lookup, BoundMethod allocation, and indirect call overhead.
        if this_val.is_heap() {
            if let Some(crate::heap::HeapObj::Array(arr)) = self.heap.get(this_val.as_heap_idx()) {
                if name.as_ref() == varn_core::MemberKey::Push.as_str() && arg_count == 1 {
                    let val = self.stack[base + arg_start];
                    arr.push_vm(val);
                    self.heap.write_barrier(this_val.as_heap_idx(), val);
                    self.stack[base + dest] = VmValue::null();
                    self.record_ic_hit_callmethod();
                    self.record_call_native(|_, _| Ok(VmValue::null()), Some("push"));
                    return Ok(false);
                } else if name.as_ref() == varn_core::MemberKey::Pop.as_str() && arg_count == 0 {
                    let val = arr.pop_vm().unwrap_or(VmValue::null());
                    self.stack[base + dest] = val;
                    self.record_ic_hit_callmethod();
                    self.record_call_native(|_, _| Ok(VmValue::null()), Some("pop"));
                    return Ok(false);
                }
            }
        }

        // Direct fast-paths for high-frequency intrinsic String methods (startsWith, indexOf).
        if this_val.is_sso() || (this_val.is_heap() && matches!(self.heap.get(this_val.as_heap_idx()), Some(crate::heap::HeapObj::Str(_)))) {
            if name.as_ref() == varn_core::MemberKey::StartsWith.as_str() && arg_count == 1 {
                let mut buf_a = [0u8; 5];
                let mut buf_b = [0u8; 5];
                let arg_val = self.stack[base + arg_start];
                let s_opt = if this_val.is_sso() {
                    Some(this_val.sso_as_str(&mut buf_a))
                } else if let Some(crate::heap::HeapObj::Str(hs)) = self.heap.get(this_val.as_heap_idx()) {
                    Some(hs.as_str())
                } else {
                    None
                };
                let p_opt = if arg_val.is_sso() {
                    Some(arg_val.sso_as_str(&mut buf_b))
                } else if arg_val.is_heap() {
                    if let Some(crate::heap::HeapObj::Str(hs)) = self.heap.get(arg_val.as_heap_idx()) {
                        Some(hs.as_str())
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let (Some(s), Some(p)) = (s_opt, p_opt) {
                    self.stack[base + dest] = VmValue::from_bool(s.starts_with(p));
                    self.record_ic_hit_callmethod();
                    return Ok(false);
                }
            } else if name.as_ref() == varn_core::MemberKey::IndexOf.as_str() && arg_count == 1 {
                let mut buf_a = [0u8; 5];
                let mut buf_b = [0u8; 5];
                let arg_val = self.stack[base + arg_start];
                let s_opt = if this_val.is_sso() {
                    Some(this_val.sso_as_str(&mut buf_a))
                } else if let Some(crate::heap::HeapObj::Str(hs)) = self.heap.get(this_val.as_heap_idx()) {
                    Some(hs.as_str())
                } else {
                    None
                };
                let p_opt = if arg_val.is_sso() {
                    Some(arg_val.sso_as_str(&mut buf_b))
                } else if arg_val.is_heap() {
                    if let Some(crate::heap::HeapObj::Str(hs)) = self.heap.get(arg_val.as_heap_idx()) {
                        Some(hs.as_str())
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let (Some(s), Some(p)) = (s_opt, p_opt) {
                    let idx = if p.is_empty() {
                        0
                    } else if let Some(byte_idx) = s.find(p) {
                        if s.is_ascii() {
                            byte_idx as i64
                        } else {
                            s[..byte_idx].chars().count() as i64
                        }
                    } else {
                        -1
                    };
                    self.stack[base + dest] = VmValue::from_int(idx);
                    self.record_ic_hit_callmethod();
                    return Ok(false);
                }
            }
        }

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
                let ic = unsafe { &*closure.ic_cache.as_ptr() };
                let poly = &ic[cs];
                let mut found_nat: Option<(NativeFnPtr, VmValue)> = None;
                let mut found_vm: Option<(Rc<VmClosure>, Option<Rc<varn_types::value::ClassObj>>)> =
                    None;
                'ic: for entry in &poly.entries {
                    if entry.id == 0 {
                        continue;
                    }
                    if entry.is_class == ICKind::NATIVE_VTABLE_METHOD {
                        if let Some(ref cls) = receiver_class {
                            if cls.id == entry.id
                                && entry.vtable_ver
                                    == (cls.vtable_version.load(Ordering::Relaxed) & 0xFF) as u8
                            {
                                let slot = entry.slot as usize;
                                let vtable = unsafe { &*cls.vtable.as_ptr() };
                                if let Some(Value::NativeFn(b)) = vtable.get(slot) {
                                    let f = b.0;
                                    found_nat = Some((f, this_val));
                                    break 'ic;
                                }
                            }
                        }
                    } else if entry.is_class == ICKind::VM_VTABLE_METHOD {
                        if let Some(ref cls) = receiver_class {
                            let cls_ver = (cls.vtable_version.load(Ordering::Relaxed) & 0xFF) as u8;
                            if cls.id == entry.id && cls_ver == entry.vtable_ver {
                                let slot = entry.slot as usize;
                                let method_val =
                                    unsafe { &*cls.vtable.as_ptr() }.get(slot).cloned();
                                if let Some(Value::VmValue(payload)) = method_val {
                                    if let Some(nc) = VmClosurePayload::downcast_from(&*payload) {
                                        let nc = nc.clone();
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
                self.record_call_native(f, Some(name.as_ref()));
                let result =
                    self.call_native_with_receiver(f, receiver, base, arg_start, arg_count)?;
                self.stack[base + dest] = result;
                return Ok(false);
            }

            if let Some((nc, owner_class)) = ic_vm {
                self.record_ic_hit_callmethod();
                return self.invoke_vm_method_fast(
                    nc,
                    owner_class,
                    this_val,
                    base,
                    arg_start,
                    arg_count,
                    dest,
                    name.as_ref(),
                );
            }

            self.record_ic_miss_callmethod();
        }

        let resolved = crate::exec::props::resolve_property(this_val, &name, &mut self.heap)?;

        if let crate::exec::props::ResolvedProperty::Built(Value::BoundMethod(bm)) = &resolved {
            if let varn_types::value::BoundMethodTarget::Native { func, .. } = &bm.target {
                let f = *func;
                let val = Value::NativeFn(Box::new((f, "")));
                self.populate_method_ic(
                    closure,
                    cs,
                    cache_len,
                    is_megamorphic,
                    &receiver_class,
                    name.as_ref(),
                    val,
                    ICKind::NATIVE_VTABLE_METHOD,
                );
                self.record_call_native(f, Some(name.as_ref()));
                let result =
                    self.call_native_with_receiver(f, this_val, base, arg_start, arg_count)?;
                self.stack[base + dest] = result;
                return Ok(false);
            }
        }

        let method_nv = match resolved {
            crate::exec::props::ResolvedProperty::Nv(v) => v,
            crate::exec::props::ResolvedProperty::Built(v) => self.heap.intern(v),
        };

        if receiver_class.is_some() {
            if cs < cache_len && method_nv.is_heap() && !is_megamorphic {
                if let Some(crate::heap::HeapObj::BoundMethod(bm)) =
                    self.heap.get(method_nv.as_heap_idx())
                {
                    match &bm.target {
                        varn_types::value::BoundMethodTarget::Native { func: f, .. } => {
                            let val = Value::NativeFn(Box::new((*f, "")));
                            self.populate_method_ic(
                                closure,
                                cs,
                                cache_len,
                                is_megamorphic,
                                &receiver_class,
                                name.as_ref(),
                                val,
                                ICKind::NATIVE_VTABLE_METHOD,
                            );
                        }
                        varn_types::value::BoundMethodTarget::Vm {
                            closure: method_closure,
                            ..
                        } => {
                            if let Some(nc) = VmClosurePayload::downcast_from(&**method_closure) {
                                if !nc.proto.is_generator && !nc.proto.is_async {
                                    let val =
                                        Value::VmValue(Box::new(VmClosurePayload(nc.clone())));
                                    self.populate_method_ic(
                                        closure,
                                        cs,
                                        cache_len,
                                        is_megamorphic,
                                        &receiver_class,
                                        name.as_ref(),
                                        val,
                                        ICKind::VM_VTABLE_METHOD,
                                    );
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
            self.record_call_native(f, Some(name.as_ref()));
            let result = self.call_native_with_receiver(f, receiver, base, arg_start, arg_count)?;
            self.stack[base + dest] = result;
            return Ok(false);
        }

        self.finish_generic_method_call(
            method_nv, this_val, base, arg_start, arg_count, dest, frame_idx,
        )
    }

    /// The generic tail of [`Self::exec_call_method_reg`]: lay the callee and
    /// arguments out on the stack and dispatch, for every method shape that has
    /// no shorter path.
    ///
    /// Split out so the enum-variant constructor can fall back into it for the
    /// one case it does not handle — see [`Self::construct_enum_variant`].
    #[allow(clippy::too_many_arguments)]
    fn finish_generic_method_call(
        &mut self,
        method_nv: VmValue,
        this_val: VmValue,
        base: usize,
        arg_start: usize,
        arg_count: usize,
        dest: usize,
        frame_idx: usize,
    ) -> VmResult<bool> {
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

    #[inline(always)]
    pub(crate) fn call_native_with_receiver(
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
            if arg_count > 0 {
                unsafe {
                    let src_ptr = self.stack.as_ptr().add(base + arg_start);
                    let dest_ptr = buf.as_mut_ptr().add(1);
                    std::ptr::copy_nonoverlapping(src_ptr, dest_ptr, arg_count);
                }
            }
            self.invoke_native(f, &buf[..arg_count + 1])
        } else {
            let mut args = Vec::with_capacity(arg_count + 1);
            args.push(receiver);
            if arg_count > 0 {
                unsafe {
                    let src_ptr = self.stack.as_ptr().add(base + arg_start);
                    let dest_ptr = args.as_mut_ptr().add(1);
                    std::ptr::copy_nonoverlapping(src_ptr, dest_ptr, arg_count);
                    args.set_len(arg_count + 1);
                }
            }
            self.invoke_native(f, &args)
        }
        .map_err(|e| RuntimeError::new(e))?;
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    fn invoke_vm_method_fast(
        &mut self,
        nc: Rc<VmClosure>,
        owner_class: Option<Rc<varn_types::value::ClassObj>>,
        this_val: VmValue,
        base: usize,
        arg_start: usize,
        arg_count: usize,
        dest: usize,
        name: &str,
    ) -> VmResult<bool> {
        self.record_call_vm_fast();
        {
            let method_key = format!(
                "{}.{}",
                owner_class.as_ref().map(|c| c.name.as_str()).unwrap_or("?"),
                name
            );
            let is_jit = nc.jit_fn().is_some();
            self.record_hotspot_method(&method_key, is_jit);
        }
        // `arity` counts register 0 (the receiver here) plus the declared
        // params, so the staged window is exactly `arity` slots wide: the
        // receiver, the supplied args, and nulls for any param the call
        // omitted.
        let nparams = nc.proto.arity.saturating_sub(1);
        let final_base = self.stack.len();
        if self.frames.len() >= 10000 {
            return Err(crate::error::RuntimeError::new(
                "stack overflow: call depth exceeded 10000",
            ));
        }
        let required = final_base + nc.proto.register_count as usize;
        if self.stack.len() < required {
            self.stack.resize(required, VmValue::null());
        }
        self.stack[final_base] = this_val;
        if !nc.proto.has_rest {
            if arg_count > 0 {
                unsafe {
                    let src = self.stack.as_ptr().add(base + arg_start);
                    let dst = self.stack.as_mut_ptr().add(final_base + 1);
                    std::ptr::copy_nonoverlapping(src, dst, arg_count);
                }
            }
        } else {
            let rest_idx = nparams.saturating_sub(1);
            let regular_count = arg_count.min(rest_idx);
            if regular_count > 0 {
                unsafe {
                    let src = self.stack.as_ptr().add(base + arg_start);
                    let dst = self.stack.as_mut_ptr().add(final_base + 1);
                    std::ptr::copy_nonoverlapping(src, dst, regular_count);
                }
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
            self.stack[final_base + 1 + rest_idx] = rest_nv;
        }
        let mut frame = crate::frame::CallFrame::new_owned(nc, final_base);
        frame.return_reg = Some(dest as u16);
        frame.caller_base = Some(base);
        frame.current_class = owner_class;
        self.frames.push(frame);
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    fn populate_method_ic(
        &self,
        closure: &VmClosure,
        cs: usize,
        cache_len: usize,
        is_megamorphic: bool,
        receiver_class: &Option<Rc<varn_types::value::ClassObj>>,
        name: &str,
        method_val: varn_types::Value,
        is_class: u8,
    ) {
        if cs >= cache_len || is_megamorphic {
            return;
        }
        // Caching only ever RECORDS what the class already has. Installing a
        // missing method here would bump `vtable_version` on the first call to
        // it, invalidating every other entry cached against that class — which
        // is what made the version guard above look wrong and get deleted.
        let _ = method_val;
        if let Some(cls) = receiver_class {
            if let Some(&slot) = cls.method_map.borrow().get(name) {
                let entry = varn_types::chunk::CacheEntry {
                    id: cls.id,
                    slot: slot as u16,
                    is_class,
                    vtable_ver: (cls.vtable_version.load(Ordering::Relaxed) & 0xFF) as u8,
                };
                closure.ic_cache.borrow_mut()[cs].find_or_insert(entry);
                closure.feedback.borrow_mut().observe(cs, cls.id);
            }
        }
    }
}
