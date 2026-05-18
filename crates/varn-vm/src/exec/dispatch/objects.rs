use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::exec::dispatch::ControlSignal;
use crate::frame::VmClosure;
use crate::value::VmValue;
use std::rc::Rc;

impl ExecCtx {
    #[inline(always)]
    pub(super) fn op_get_property(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        closure: &VmClosure,
        frame_idx: usize,
    ) -> VmResult<ControlSignal> {
        let idx = code[*ip] as usize;
        *ip += 1;
        let cs_idx = code[*ip] as usize;
        *ip += 1;

        let name = self.read_str_const_at(idx, frame_idx)?;
        let obj = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };

        let mut ic_hit_val: Option<VmValue> = None;
        let mut ic_signal: Option<ControlSignal> = None;
        let cache_len = closure.ic_cache_len();
        let obj_is_heap_object = obj.is_heap()
            && matches!(
                self.heap.get(obj.as_heap_idx()),
                Some(crate::heap::HeapObj::Object(_))
            );
        if obj_is_heap_object && cs_idx < cache_len {
            let slot_cache = closure.ic_cache.borrow();
            let poly_slot = &slot_cache[cs_idx];
            let mut hit_found = false;

            for entry in &poly_slot.entries {
                if entry.id == 0 {
                    continue;
                }

                let mut found_method: Option<(
                    varn_types::Value,
                    Option<Rc<varn_types::ClassObj>>,
                )> = None;
                let mut found_getter: Option<varn_types::Value> = None;
                let mut found_slot_val: Option<VmValue> = None;

                if entry.is_class == 1 {
                    if obj.is_heap() {
                        if let Some(crate::heap::HeapObj::Object(o)) =
                            self.heap.get(obj.as_heap_idx())
                        {
                            let guard = o.borrow();
                            if guard.inner.shape.id == entry.id {
                                let slot = entry.slot as usize;
                                if slot < guard.inner.values.len() {
                                    found_slot_val = Some(guard.inner.values[slot]);
                                }
                            }
                        }
                    }
                } else if let Some(cls) = crate::exec::props::get_class(obj, &self.heap) {
                    let slot = entry.slot as usize;
                    if entry.is_class == 2 && cls.id == entry.id {
                        let vtable = cls.vtable.borrow();
                        let vtable_owners = cls.vtable_owners.borrow();
                        if slot < vtable.len() {
                            found_method =
                                Some((vtable[slot].clone(), vtable_owners[slot].clone()));
                        }
                    } else if entry.is_class == 3 && cls.id == entry.id {
                        let vtable = cls.getter_vtable.borrow();
                        if slot < vtable.len() {
                            found_getter = Some(vtable[slot].clone());
                        }
                    }
                }

                if let Some(v) = found_slot_val {
                    self.record_ic_hit_getprop();
                    ic_hit_val = Some(v);
                    hit_found = true;
                    break;
                } else if let Some((method, owner)) = found_method {
                    self.record_ic_hit_getprop();
                    let receiver = self.heap.extract(obj);
                    let bound =
                        crate::exec::props::bind_method_to_receiver(receiver, method, owner);
                    ic_hit_val = Some(self.heap.intern(bound));
                    hit_found = true;
                    break;
                } else if let Some(getter_val) = found_getter {
                    self.record_ic_hit_getprop();
                    let callee_nv = self.heap.intern(getter_val);
                    self.stack.push(callee_nv);
                    self.stack.push(obj);
                    self.frames[frame_idx].ip = *ip;
                    let prepared = self.prepare_call(callee_nv, 1)?;
                    let depth = self.frames.len();
                    self.dispatch_prepared_call(prepared)?;
                    if self.frames.len() > depth {
                        ic_signal = Some(ControlSignal::ContinueFrame);
                    } else {
                        ic_signal = Some(ControlSignal::ContinueInstruction);
                    }
                    hit_found = true;
                    break;
                }
            }

            if !hit_found {
                self.record_ic_miss_getprop();
            }
            drop(slot_cache);
        }
        if let Some(sig) = ic_signal {
            Ok(sig)
        } else if let Some(v) = ic_hit_val {
            self.stack.push(v);
            Ok(ControlSignal::ContinueInstruction)
        } else {
            if let Some(getter_val) = crate::exec::props::find_getter(obj, &name, &self.heap) {
                let callee_nv = self.heap.intern(getter_val);
                self.stack.push(callee_nv);
                self.stack.push(obj);
                self.frames[frame_idx].ip = *ip;

                if cs_idx < closure.ic_cache_len() {
                    if let Some(cls) = crate::exec::props::get_class(obj, &self.heap) {
                        if let Some(&slot) = cls.getter_map.borrow().get(name.as_ref()) {
                            let entry = varn_types::chunk::CacheEntry {
                                id: cls.id,
                                slot: slot as u16,
                                is_class: 3,
                                vtable_ver: 0,
                            };
                            closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                        }
                    }
                }

                let prepared = self.prepare_call(callee_nv, 1)?;
                let depth = self.frames.len();
                self.dispatch_prepared_call(prepared)?;
                if self.frames.len() > depth {
                    Ok(ControlSignal::ContinueFrame)
                } else {
                    Ok(ControlSignal::ContinueInstruction)
                }
            } else {
                let v = crate::exec::props::get_property(obj, &name, &mut self.heap)?;
                self.stack.push(v);
                if cs_idx < closure.ic_cache_len() {
                    if obj.is_heap() {
                        if let Some(crate::heap::HeapObj::Object(o)) =
                            self.heap.get(obj.as_heap_idx())
                        {
                            let guard = o.borrow();
                            if let Some(&slot) = guard.inner.shape.property_names.get(name.as_ref())
                            {
                                if slot < guard.inner.values.len() {
                                    let entry = varn_types::chunk::CacheEntry {
                                        id: guard.inner.shape.id,
                                        slot: slot as u16,
                                        is_class: 1,
                                        vtable_ver: 0,
                                    };
                                    closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                                }
                                return Ok(ControlSignal::ContinueInstruction);
                            }
                        }
                    }

                    if let Some(cls) = crate::exec::props::get_class(obj, &self.heap) {
                        if let Some(&slot) = cls.method_map.borrow().get(name.as_ref()) {
                            let entry = varn_types::chunk::CacheEntry {
                                id: cls.id,
                                slot: slot as u16,
                                is_class: 2,
                                vtable_ver: 0,
                            };
                            closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                        }
                    }
                }
                Ok(ControlSignal::ContinueInstruction)
            }
        }
    }

    #[inline(always)]
    pub(super) fn op_set_property(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        closure: &VmClosure,
        frame_idx: usize,
    ) -> VmResult<ControlSignal> {
        let idx = code[*ip] as usize;
        *ip += 1;
        let cs_idx = code[*ip] as usize;
        *ip += 1;

        let name = self.read_str_const_at(idx, frame_idx)?;
        let len = self.stack.len();
        let val = self.stack[len - 1];
        let obj = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };

        let mut used_ic = false;
        let mut ic_signal: Option<ControlSignal> = None;

        let obj_is_heap_object = obj.is_heap()
            && matches!(
                self.heap.get(obj.as_heap_idx()),
                Some(crate::heap::HeapObj::Object(_))
            );
        if obj_is_heap_object && cs_idx < closure.ic_cache_len() {
            let slot_cache = closure.ic_cache.borrow();
            let poly_slot = &slot_cache[cs_idx];
            let mut hit_found = false;

            for entry in &poly_slot.entries {
                if entry.id == 0 {
                    continue;
                }

                let mut found_slot: Option<usize> = None;
                let mut found_setter: Option<varn_types::Value> = None;

                if let Some(crate::heap::HeapObj::Object(o)) = if obj.is_heap() {
                    self.heap.get(obj.as_heap_idx())
                } else {
                    None
                } {
                    let guard = o.borrow();
                    let slot = entry.slot as usize;
                    if entry.is_class == 1 && guard.inner.shape.id == entry.id {
                        if slot < guard.inner.values.len() {
                            found_slot = Some(slot);
                        }
                    } else if entry.is_class == 5 && guard.inner.shape.id == entry.id {
                        found_slot = Some(slot);
                    }
                }

                if found_slot.is_none() {
                    if let Some(cls) = crate::exec::props::get_class(obj, &self.heap) {
                        let slot = entry.slot as usize;
                        if entry.is_class == 4 && cls.id == entry.id {
                            let vtable = cls.setter_vtable.borrow();
                            if slot < vtable.len() {
                                found_setter = Some(vtable[slot].clone());
                            }
                        }
                    }
                }

                if let Some(slot) = found_slot {
                    self.record_ic_hit_setprop();
                    if let Some(crate::heap::HeapObj::Object(o)) = self.heap.get(obj.as_heap_idx())
                    {
                        let mut o_guard = o.borrow_mut();
                        if entry.is_class == 5 {
                            o_guard.inner.insert(Rc::from(name.as_ref()), val);
                        } else {
                            o_guard.inner.values[slot] = val;
                        }
                        used_ic = true;
                        hit_found = true;
                        break;
                    }
                } else if let Some(setter_val) = found_setter {
                    self.record_ic_hit_setprop();
                    let callee_nv = self.heap.intern(setter_val);
                    self.stack.push(callee_nv);
                    self.stack.push(obj);
                    self.stack.push(val);
                    self.frames[frame_idx].ip = *ip;
                    let prepared = self.prepare_call(callee_nv, 2)?;
                    self.dispatch_prepared_setter_call(prepared, val)?;
                    ic_signal = Some(ControlSignal::ContinueFrame);
                    used_ic = true;
                    hit_found = true;
                    break;
                }
            }

            if !hit_found {
                self.record_ic_miss_setprop();
            }
            drop(slot_cache);
        }

        if let Some(sig) = ic_signal {
            Ok(sig)
        } else {
            if !used_ic {
                if obj.is_heap() {
                    if let Some(crate::heap::HeapObj::Object(o)) = self.heap.get(obj.as_heap_idx())
                    {
                        let mut guard = o.borrow_mut();
                        if let Some(&slot) = guard.inner.shape.property_names.get(name.as_ref()) {
                            if slot < guard.inner.values.len() {
                                guard.inner.values[slot] = val;
                                if cs_idx < closure.ic_cache_len() {
                                    let entry = varn_types::chunk::CacheEntry {
                                        id: guard.inner.shape.id,
                                        slot: slot as u16,
                                        is_class: 1,
                                        vtable_ver: 0,
                                    };
                                    closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                                }
                                used_ic = true;
                            }
                        }
                    }
                }
            }

            if !used_ic {
                let setter_val = crate::exec::props::find_setter(obj, &name, &self.heap);
                if let Some(setter_val) = setter_val {
                    let callee_nv = self.heap.intern(setter_val);
                    self.stack.push(callee_nv);
                    self.stack.push(obj);
                    self.stack.push(val);
                    self.frames[frame_idx].ip = *ip;

                    if cs_idx < closure.ic_cache_len() {
                        if let Some(cls) = crate::exec::props::get_class(obj, &self.heap) {
                            if let Some(&slot) = cls.setter_map.borrow().get(name.as_ref()) {
                                let entry = varn_types::chunk::CacheEntry {
                                    id: cls.id,
                                    slot: slot as u16,
                                    is_class: 4,
                                    vtable_ver: 0,
                                };
                                closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                            }
                        }
                    }

                    let prepared = self.prepare_call(callee_nv, 2)?;
                    self.dispatch_prepared_setter_call(prepared, val)?;
                    return Ok(ControlSignal::ContinueFrame);
                } else {
                    if obj.is_heap() {
                        match self.heap.get(obj.as_heap_idx()).cloned() {
                            Some(crate::heap::HeapObj::Object(o)) => {
                                let mut guard = o.borrow_mut();
                                let old_shape_id = guard.inner.shape.id;
                                guard.inner.insert(Rc::from(name.as_ref()), val);
                                if cs_idx < closure.ic_cache_len() {
                                    let entry = varn_types::chunk::CacheEntry {
                                        id: old_shape_id,
                                        slot: (guard.inner.values.len() - 1) as u16,
                                        is_class: 5,
                                        vtable_ver: 0,
                                    };
                                    closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                                }
                                used_ic = true;
                            }
                            _ => {
                                crate::exec::props::set_property(obj, &name, val, &mut self.heap)?;
                                used_ic = true;
                            }
                        }
                    }
                }
            }
            if used_ic {
                self.stack.push(val);
                Ok(ControlSignal::ContinueInstruction)
            } else {
                crate::exec::props::set_property(obj, &name, val, &mut self.heap)?;
                self.stack.push(val);
                Ok(ControlSignal::ContinueInstruction)
            }
        }
    }

    #[inline(always)]
    pub(super) fn op_get_property_maybe(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        closure: &VmClosure,
        frame_idx: usize,
    ) -> VmResult<ControlSignal> {
        let idx = code[*ip] as usize;
        *ip += 1;
        let cs_idx = code[*ip] as usize;
        *ip += 1;

        let name = self.read_str_const_at(idx, frame_idx)?;
        let obj = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };

        let mut ic_hit_val: Option<VmValue> = None;
        let mut ic_signal: Option<ControlSignal> = None;
        if cs_idx < closure.ic_cache_len() {
            let slot_cache = closure.ic_cache.borrow();
            let poly_slot = &slot_cache[cs_idx];
            let mut hit_found = false;

            for entry in &poly_slot.entries {
                if entry.id == 0 {
                    continue;
                }

                let mut found_getter: Option<varn_types::Value> = None;
                let mut found_method: Option<(
                    varn_types::Value,
                    Option<Rc<varn_types::ClassObj>>,
                )> = None;
                let mut found_slot_val: Option<VmValue> = None;

                if entry.is_class == 1 {
                    if obj.is_heap() {
                        if let Some(crate::heap::HeapObj::Object(o)) =
                            self.heap.get(obj.as_heap_idx())
                        {
                            let guard = o.borrow();
                            if guard.inner.shape.id == entry.id {
                                let slot = entry.slot as usize;
                                if slot < guard.inner.values.len() {
                                    found_slot_val = Some(guard.inner.values[slot]);
                                }
                            }
                        }
                    }
                } else if let Some(cls) = crate::exec::props::get_class(obj, &self.heap) {
                    let slot = entry.slot as usize;
                    if entry.is_class == 2 && cls.id == entry.id {
                        let vtable = cls.vtable.borrow();
                        let vtable_owners = cls.vtable_owners.borrow();
                        if slot < vtable.len() {
                            found_method =
                                Some((vtable[slot].clone(), vtable_owners[slot].clone()));
                        }
                    } else if entry.is_class == 3 && cls.id == entry.id {
                        let vtable = cls.getter_vtable.borrow();
                        if slot < vtable.len() {
                            found_getter = Some(vtable[slot].clone());
                        }
                    }
                }

                if let Some(v) = found_slot_val {
                    self.record_ic_hit();
                    ic_hit_val = Some(v);
                    hit_found = true;
                    break;
                } else if let Some((method, owner)) = found_method {
                    self.record_ic_hit();
                    let receiver = self.heap.extract(obj);
                    ic_hit_val = Some(self.heap.intern(
                        crate::exec::props::bind_method_to_receiver(receiver, method, owner),
                    ));
                    hit_found = true;
                    break;
                } else if let Some(getter_val) = found_getter {
                    self.record_ic_hit();
                    let callee_nv = self.heap.intern(getter_val);
                    self.stack.push(callee_nv);
                    self.stack.push(obj);
                    self.frames[frame_idx].ip = *ip;
                    let prepared = self.prepare_call(callee_nv, 1)?;
                    let depth = self.frames.len();
                    self.dispatch_prepared_call(prepared)?;
                    if self.frames.len() > depth {
                        ic_signal = Some(ControlSignal::ContinueFrame);
                    } else {
                        ic_signal = Some(ControlSignal::ContinueInstruction);
                    }
                    hit_found = true;
                    break;
                }
            }

            if !hit_found {
                self.record_ic_miss();
            }
            drop(slot_cache);
        }
        if let Some(sig) = ic_signal {
            Ok(sig)
        } else if let Some(v) = ic_hit_val {
            self.stack.push(v);
            Ok(ControlSignal::ContinueInstruction)
        } else {
            if let Some(getter_val) = crate::exec::props::find_getter(obj, &name, &self.heap) {
                let callee_nv = self.heap.intern(getter_val);
                self.stack.push(callee_nv);
                self.stack.push(obj);
                self.frames[frame_idx].ip = *ip;

                if cs_idx < closure.ic_cache_len() {
                    if let Some(cls) = crate::exec::props::get_class(obj, &self.heap) {
                        if let Some(&slot) = cls.getter_map.borrow().get(name.as_ref()) {
                            let entry = varn_types::chunk::CacheEntry {
                                id: cls.id,
                                slot: slot as u16,
                                is_class: 3,
                                vtable_ver: 0,
                            };
                            closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                        }
                    }
                }

                let prepared = self.prepare_call(callee_nv, 1)?;
                let depth = self.frames.len();
                self.dispatch_prepared_call(prepared)?;
                if self.frames.len() > depth {
                    Ok(ControlSignal::ContinueFrame)
                } else {
                    Ok(ControlSignal::ContinueInstruction)
                }
            } else {
                let v = crate::exec::props::get_property_maybe(obj, &name, &mut self.heap);
                self.stack.push(v);
                if cs_idx < closure.ic_cache_len() {
                    if obj.is_heap() {
                        if let Some(crate::heap::HeapObj::Object(o)) =
                            self.heap.get(obj.as_heap_idx())
                        {
                            let guard = o.borrow();
                            if let Some(&slot) = guard.inner.shape.property_names.get(name.as_ref())
                            {
                                if slot < guard.inner.values.len() {
                                    let entry = varn_types::chunk::CacheEntry {
                                        id: guard.inner.shape.id,
                                        slot: slot as u16,
                                        is_class: 1,
                                        vtable_ver: 0,
                                    };
                                    closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                                }
                                return Ok(ControlSignal::ContinueInstruction);
                            }
                        }
                    }

                    if let Some(cls) = crate::exec::props::get_class(obj, &self.heap) {
                        if let Some(&slot) = cls.method_map.borrow().get(name.as_ref()) {
                            let entry = varn_types::chunk::CacheEntry {
                                id: cls.id,
                                slot: slot as u16,
                                is_class: 2,
                                vtable_ver: 0,
                            };
                            closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                        }
                    }
                }
                Ok(ControlSignal::ContinueInstruction)
            }
        }
    }

    #[inline(always)]
    pub(super) fn op_get_index(&mut self) -> VmResult<ControlSignal> {
        let len = self.stack.len();
        let b = self.stack[len - 1];
        let a = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let v = crate::exec::collections::get_index(a, b, &mut self.heap)?;
        self.stack.push(v);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_set_index(&mut self) -> VmResult<ControlSignal> {
        let len = self.stack.len();
        let val = self.stack[len - 1];
        let idx = self.stack[len - 2];
        let obj = self.stack[len - 3];
        unsafe { self.stack.set_len(len - 3) };
        crate::exec::collections::set_index(obj, idx, val, &mut self.heap)?;
        self.stack.push(val);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_build_object(&mut self, code: &[u16], ip: &mut usize) -> ControlSignal {
        let count = code[*ip] as usize;
        *ip += 1;
        let nv = crate::exec::collections::build_object(&mut self.stack, count, &mut self.heap);
        self.stack.push(nv);
        ControlSignal::ContinueInstruction
    }

    pub(super) fn op_build_object_with_shape(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        closure: &crate::frame::VmClosure,
    ) -> VmResult<ControlSignal> {
        let shape_idx = code[*ip] as usize;
        *ip += 1;
        let keys = match closure.proto.chunk.constants.get(shape_idx) {
            Some(varn_types::PoolEntry::Shape(keys)) => keys,
            _ => {
                return Err(RuntimeError::new(
                    "OpBuildObjectWithShape: expected shape in constant pool",
                ))
            }
        };
        let nv = crate::exec::collections::build_object_with_shape(
            &mut self.stack,
            keys,
            &mut self.heap,
        );
        self.stack.push(nv);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_build_array(&mut self, code: &[u16], ip: &mut usize) -> ControlSignal {
        let count = code[*ip] as usize;
        *ip += 1;
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            items.push(self.heap.extract(unsafe {
                let len = self.stack.len() - 1;
                let v = self.stack[len];
                self.stack.set_len(len);
                v
            }));
        }
        items.reverse();
        let arr = self.heap.alloc_array(items);
        self.stack.push(arr);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_is_array(&mut self) -> ControlSignal {
        let v = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        self.stack.push(VmValue::from_bool(
            v.is_heap()
                && matches!(
                    self.heap.get(v.as_heap_idx()),
                    Some(crate::heap::HeapObj::Array(_))
                ),
        ));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_is_null(&mut self) -> ControlSignal {
        let val = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        self.stack
            .push(VmValue::from_bool(crate::exec::advanced::is_null(val)));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_typeof(&mut self) -> ControlSignal {
        let v = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let t = crate::exec::advanced::typeof_val(v, &self.heap);
        let nv = self.heap.alloc_str(t);
        self.stack.push(nv);
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_in(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let obj = self.stack[len - 1];
        let key = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        let res = crate::exec::advanced::op_in(key, obj, &self.heap);
        self.stack.push(VmValue::from_bool(res));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_class(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
    ) -> VmResult<ControlSignal> {
        let name_idx = code[*ip] as usize;
        *ip += 1;
        let name = self.read_str_const_at(name_idx, frame_idx)?;
        let cls = crate::exec::class::op_class(&name, &mut self.heap);
        self.stack.push(cls);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_method(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
    ) -> VmResult<ControlSignal> {
        let name_idx = code[*ip] as usize;
        *ip += 1;
        let name = self.read_str_const_at(name_idx, frame_idx)?;
        let method = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let cls = self.stack[self.stack.len() - 1];
        crate::exec::class::op_method(cls, &name, method, &mut self.heap)?;
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_inherit(&mut self) -> VmResult<ControlSignal> {
        let len = self.stack.len();
        let sub = self.stack[len - 1];
        let sup = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        crate::exec::class::op_inherit(sub, sup, &mut self.heap)?;
        self.stack.push(sub);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_define_static(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
    ) -> VmResult<ControlSignal> {
        let idx = code[*ip] as usize;
        *ip += 1;
        let name = self.read_str_const_at(idx, frame_idx)?;
        let val = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let cls = self.stack[self.stack.len() - 1];
        crate::exec::class::op_define_static(cls, &name, val, &mut self.heap)?;
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_define_getter(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
    ) -> VmResult<ControlSignal> {
        let idx = code[*ip] as usize;
        *ip += 1;
        let name = self.read_str_const_at(idx, frame_idx)?;
        let getter = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let cls = self.stack[self.stack.len() - 1];
        crate::exec::class::op_define_getter(cls, &name, getter, &mut self.heap)?;
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_define_setter(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
    ) -> VmResult<ControlSignal> {
        let idx = code[*ip] as usize;
        *ip += 1;
        let name = self.read_str_const_at(idx, frame_idx)?;
        let setter = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let cls = self.stack[self.stack.len() - 1];
        crate::exec::class::op_define_setter(cls, &name, setter, &mut self.heap)?;
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_define_static_getter(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
    ) -> VmResult<ControlSignal> {
        let idx = code[*ip] as usize;
        *ip += 1;
        let name = self.read_str_const_at(idx, frame_idx)?;
        let getter = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let cls = self.stack[self.stack.len() - 1];
        crate::exec::class::op_define_static_getter(cls, &name, getter, &mut self.heap)?;
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_define_static_setter(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
    ) -> VmResult<ControlSignal> {
        let idx = code[*ip] as usize;
        *ip += 1;
        let name = self.read_str_const_at(idx, frame_idx)?;
        let setter = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let cls = self.stack[self.stack.len() - 1];
        crate::exec::class::op_define_static_setter(cls, &name, setter, &mut self.heap)?;
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_declare_field(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
    ) -> VmResult<ControlSignal> {
        let idx = code[*ip] as usize;
        *ip += 1;
        let name = self.read_str_const_at(idx, frame_idx)?;
        let cls = self.stack[self.stack.len() - 1];
        crate::exec::class::op_declare_field(cls, &name, &mut self.heap)?;
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_get_fixed_field(
        &mut self,
        code: &[u16],
        ip: &mut usize,
    ) -> VmResult<ControlSignal> {
        let slot = code[*ip] as usize;
        *ip += 1;
        let obj = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let nv = crate::exec::props::get_fixed_field(obj, slot, &mut self.heap)?;
        self.stack.push(nv);
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_set_fixed_field(
        &mut self,
        code: &[u16],
        ip: &mut usize,
    ) -> VmResult<ControlSignal> {
        let slot = code[*ip] as usize;
        *ip += 1;
        let val = unsafe {
            let len = self.stack.len() - 1;
            let v = self.stack[len];
            self.stack.set_len(len);
            v
        };
        let obj = self.stack[self.stack.len() - 1];
        crate::exec::props::set_fixed_field(obj, slot, val, &mut self.heap)?;
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_instanceof(&mut self) -> ControlSignal {
        let len = self.stack.len();
        let cls = self.stack[len - 1];
        let obj = self.stack[len - 2];
        unsafe { self.stack.set_len(len - 2) };
        self.stack
            .push(VmValue::from_bool(crate::exec::advanced::instanceof(
                obj, cls, &self.heap,
            )));
        ControlSignal::ContinueInstruction
    }

    #[inline(always)]
    pub(super) fn op_assert_not_null(&mut self) -> VmResult<ControlSignal> {
        let v = self.stack[self.stack.len() - 1];
        crate::exec::advanced::assert_not_null(v)?;
        Ok(ControlSignal::ContinueInstruction)
    }

    #[inline(always)]
    pub(super) fn op_get_super(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        frame_idx: usize,
        base: usize,
    ) -> VmResult<ControlSignal> {
        let name_idx = code[*ip] as usize;
        *ip += 1;
        let name = self.read_str_const_at(name_idx, frame_idx)?;
        let lookup = if name.as_ref() == "super" {
            "constructor"
        } else {
            name.as_ref()
        };
        let current_class = self.frames[frame_idx].current_class.clone();
        let (this_val, super_cls) = {
            let this_nv = *self.stack.get(base).unwrap_or(&VmValue::null());
            let this = self.heap.extract(this_nv);
            let super_cls = current_class
                .as_ref()
                .and_then(|c| c.superclass.borrow().clone());
            (this, super_cls)
        };
        let method_val = super_cls
            .as_ref()
            .and_then(|s| varn_types::value::find_method_with_owner(s, lookup));
        let bound = match method_val {
            Some((method, owner)) => {
                crate::exec::props::bind_method_to_receiver(this_val, method, Some(owner))
            }
            _ => varn_types::Value::native(|_, _| Ok(VmValue::null()), "super"),
        };
        let nv = self.heap.intern(bound);
        self.stack.push(nv);
        Ok(ControlSignal::ContinueInstruction)
    }
}
