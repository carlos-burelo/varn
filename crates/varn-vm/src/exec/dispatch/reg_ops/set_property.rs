use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::frame::VmClosure;
use crate::value::VmValue;
use std::rc::Rc;
use std::sync::atomic::Ordering;

impl ExecCtx {
    pub(crate) fn exec_set_property_reg(
        &mut self,
        obj: VmValue,
        val: VmValue,
        name_idx: usize,
        cs_idx: usize,
        base: usize,
        _frame_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<bool> {
        let name_nv = closure.constants[name_idx];
        let name = self
            .heap
            .str_val(name_nv)
            .ok_or_else(|| RuntimeError::new("SetProperty: non-string const"))?;

        let is_megamorphic = closure
            .feedback
            .borrow()
            .sites
            .get(cs_idx)
            .map(|s| s.megamorphic)
            .unwrap_or(false);

        let cache_len = closure.ic_cache_len();
        let obj_is_heap_object = obj.is_heap()
            && matches!(
                self.heap.get(obj.as_heap_idx()),
                Some(crate::heap::HeapObj::Object(_))
            );
        if obj_is_heap_object && cs_idx < cache_len && !is_megamorphic {
            let mut found_slot: Option<(usize, u8)> = None;
            let mut found_setter: Option<varn_types::Value> = None;
            let mut hit_found = false;

            {
                let slot_cache = unsafe { &*closure.ic_cache.as_ptr() };
                let poly_slot = &slot_cache[cs_idx];

                'entries: for entry in &poly_slot.entries {
                    if entry.id == 0 {
                        continue;
                    }

                    let slot = entry.slot as usize;
                    if obj.is_heap() {
                        if let Some(crate::heap::HeapObj::Object(o)) =
                            self.heap.get(obj.as_heap_idx())
                        {
                            let guard = unsafe { &*o.0.as_ptr() };
                            if entry.is_class == 1
                                && guard.inner.shape.id == entry.id
                                && slot < guard.inner.values.len()
                            {
                                found_slot = Some((slot, 1));
                                hit_found = true;
                                break 'entries;
                            } else if entry.is_class == 5 && guard.inner.shape.id == entry.id {
                                found_slot = Some((slot, 5));
                                hit_found = true;
                                break 'entries;
                            }
                        }
                    }

                    if found_slot.is_none() {
                        if let Some(cls) = crate::exec::props::get_class(obj, &self.heap) {
                            if entry.is_class == 4
                                && cls.id == entry.id
                                && entry.vtable_ver
                                    == (cls.vtable_version.load(Ordering::Relaxed) & 0xFF) as u8
                            {
                                let vtable = unsafe { &*cls.setter_vtable.as_ptr() };
                                if slot < vtable.len() {
                                    found_setter = Some(vtable[slot].clone());
                                    hit_found = true;
                                    break 'entries;
                                }
                            }
                        }
                    }
                }

                if !hit_found {
                    self.record_ic_miss_setprop();
                }
            }

            if let Some((slot, kind)) = found_slot {
                self.record_ic_hit_setprop();
                if obj.is_heap() {
                    if let Some(crate::heap::HeapObj::Object(o)) = self.heap.get(obj.as_heap_idx())
                    {
                        let guard = unsafe { &mut *o.0.as_ptr() };
                        if kind == 5 {
                            guard.inner.insert(Rc::from(name.as_ref()), val);
                        } else {
                            guard.inner.values[slot] = val;
                        }
                        return Ok(false);
                    }
                }
            } else if let Some(setter_val) = found_setter {
                self.record_ic_hit_setprop();
                let setter_nv = self.heap.intern(setter_val);
                self.call_setter_sync(setter_nv, obj, val, base)?;
                return Ok(false);
            }
        }

        if let Some(setter_val) = crate::exec::props::find_setter(obj, &name, &self.heap) {
            if cs_idx < cache_len && !is_megamorphic {
                if let Some(cls) = crate::exec::props::get_class(obj, &self.heap) {
                    if let Some(&slot) = cls.setter_map.borrow().get(name.as_ref()) {
                        let entry = varn_types::chunk::CacheEntry {
                            id: cls.id,
                            slot: slot as u16,
                            is_class: 4,
                            vtable_ver: (cls.vtable_version.load(Ordering::Relaxed) & 0xFF) as u8,
                        };
                        closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                        closure.feedback.borrow_mut().observe(cs_idx, cls.id);
                    }
                }
            }
            let setter_nv = self.heap.intern(setter_val);
            self.call_setter_sync(setter_nv, obj, val, base)?;
            return Ok(false);
        }

        if obj.is_heap() {
            if let Some(crate::heap::HeapObj::Object(o)) = self.heap.get(obj.as_heap_idx()) {
                let mut guard = o.borrow_mut();
                if let Some(&slot) = guard.inner.shape.property_names.get(name.as_ref()) {
                    if slot < guard.inner.values.len() {
                        let entry = varn_types::chunk::CacheEntry {
                            id: guard.inner.shape.id,
                            slot: slot as u16,
                            is_class: 1,
                            vtable_ver: 0,
                        };
                        guard.inner.values[slot] = val;
                        let shape_id = guard.inner.shape.id;
                        drop(guard);
                        if cs_idx < cache_len && !is_megamorphic {
                            closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                            closure.feedback.borrow_mut().observe(cs_idx, shape_id);
                        }
                        return Ok(false);
                    }
                }

                let old_shape_id = guard.inner.shape.id;
                guard.inner.insert(Rc::from(name.as_ref()), val);
                let new_slot = guard.inner.values.len().saturating_sub(1);
                let new_shape_id = guard.inner.shape.id;
                drop(guard);
                if cs_idx < cache_len && !is_megamorphic {
                    let mut ic = closure.ic_cache.borrow_mut();

                    ic[cs_idx].find_or_insert(varn_types::chunk::CacheEntry {
                        id: old_shape_id,
                        slot: new_slot as u16,
                        is_class: 5,
                        vtable_ver: 0,
                    });

                    ic[cs_idx].find_or_insert(varn_types::chunk::CacheEntry {
                        id: new_shape_id,
                        slot: new_slot as u16,
                        is_class: 1,
                        vtable_ver: 0,
                    });
                    drop(ic);
                    closure.feedback.borrow_mut().observe(cs_idx, old_shape_id);
                }
                return Ok(false);
            }
        }

        crate::exec::props::set_property(obj, &name, val, &mut self.heap)?;
        Ok(false)
    }

    pub(in crate::exec::dispatch) fn call_setter_sync(
        &mut self,
        setter_nv: VmValue,
        receiver: VmValue,
        value: VmValue,
        _base: usize,
    ) -> VmResult<()> {
        let setter_val = self.heap.extract(setter_nv);
        match setter_val {
            varn_types::Value::NativeFn(boxed) => {
                let f = boxed.0;
                let args = [receiver, value];
                f(self as &mut dyn varn_types::NativeCtx, &args)
                    .map_err(|e| crate::error::RuntimeError::new(e))?;
            }
            varn_types::Value::VmValue(_) | varn_types::Value::BoundMethod(_) => {
                let setter_nv2 = self.heap.intern(setter_val);
                self.stack.push(setter_nv2);
                self.stack.push(receiver);
                self.stack.push(value);
                let prepared = self
                    .prepare_call(setter_nv2, 2)
                    .map_err(|e| crate::error::RuntimeError::new(e.message))?;
                if let crate::exec::calls::PreparedCall::Frame(mut frame) = prepared {
                    if self.frames.len() >= 10000 {
                        return Err(crate::error::RuntimeError::new(
                            "stack overflow: call depth exceeded 10000",
                        ));
                    }
                    frame.return_reg = None;
                    let required = frame.base + frame.closure().proto.register_count as usize;
                    if self.stack.len() < required {
                        self.stack.resize(required, VmValue::null());
                    }
                    self.record_frame_push();
                    self.frames.push(frame);
                    let depth = self.frames.len() - 1;
                    self.run_until(depth)
                        .map_err(|e| crate::error::RuntimeError::new(e.message))?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(in crate::exec::dispatch) fn call_getter_sync(
        &mut self,
        getter_nv: VmValue,
        receiver: VmValue,
        dest: usize,
        base: usize,
        _frame_idx: usize,
    ) -> VmResult<bool> {
        let getter_val = self.heap.extract(getter_nv);
        let result_opt: Option<varn_types::Value> = match getter_val {
            varn_types::Value::NativeFn(boxed) => {
                let f = boxed.0;
                let args = [receiver];
                let nv = f(self as &mut dyn varn_types::NativeCtx, &args)
                    .map_err(|e| crate::error::RuntimeError::new(e))?;
                Some(self.heap.extract(nv))
            }
            varn_types::Value::VmValue(_) | varn_types::Value::BoundMethod(_) => {
                let getter_nv2 = self.heap.intern(getter_val);
                self.stack.push(getter_nv2);
                self.stack.push(receiver);
                let prepared = self
                    .prepare_call(getter_nv2, 1)
                    .map_err(|e| crate::error::RuntimeError::new(e.message))?;
                match prepared {
                    crate::exec::calls::PreparedCall::Frame(mut frame) => {
                        if self.frames.len() >= 10000 {
                            return Err(crate::error::RuntimeError::new(
                                "stack overflow: call depth exceeded 10000",
                            ));
                        }
                        frame.return_reg = Some(dest as u16);
                        let required = frame.base + frame.closure().proto.register_count as usize;
                        if self.stack.len() < required {
                            self.stack.resize(required, VmValue::null());
                        }
                        self.record_frame_push();
                        self.frames.push(frame);
                        return Ok(true);
                    }
                    crate::exec::calls::PreparedCall::Native(f, args) => {
                        let nv = f(self as &mut dyn varn_types::NativeCtx, &args)
                            .map_err(|e| crate::error::RuntimeError::new(e))?;
                        self.stack.pop();
                        Some(self.heap.extract(nv))
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(result) = result_opt {
            let result_nv = self.heap.intern(result);
            self.stack[base + dest] = result_nv;
        }
        Ok(false)
    }
}
