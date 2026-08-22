use crate::closure::VmClosure;
use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use varn_types::chunk::ICKind;

impl ExecCtx {
    pub(crate) fn exec_get_property_reg(
        &mut self,
        obj: VmValue,
        name_idx: usize,
        cs_idx: usize,
        dest: usize,
        base: usize,
        frame_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<bool> {
        let name_nv = closure.constants[name_idx];
        let name = self
            .heap
            .str_val(name_nv)
            .ok_or_else(|| RuntimeError::new("GetProperty: non-string const"))?;

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
                Some(crate::heap::HeapObj::Object(_) | crate::heap::HeapObj::Record(_))
            );
        if obj_is_heap_object && cs_idx < cache_len && !is_megamorphic {
            let mut found_slot_val: Option<VmValue> = None;
            let mut found_method: Option<(varn_types::Value, Option<Rc<varn_types::ClassObj>>)> =
                None;
            let mut found_getter: Option<varn_types::Value> = None;
            let mut hit_found = false;

            {
                let slot_cache = unsafe { &*closure.ic_cache.as_ptr() };
                let poly_slot = &slot_cache[cs_idx];

                'entries: for entry in &poly_slot.entries {
                    if entry.id == 0 {
                        continue;
                    }

                    if entry.is_class == ICKind::SHAPE_PROP {
                        if obj.is_heap() {
                            if let Some(
                                crate::heap::HeapObj::Object(o) | crate::heap::HeapObj::Record(o),
                            ) = self.heap.get(obj.as_heap_idx())
                            {
                                let guard = o.read();
                                if guard.shape().id == entry.id {
                                    if let Some(v) = guard.field_at(entry.slot as usize) {
                                        found_slot_val = Some(v);
                                        hit_found = true;
                                        break 'entries;
                                    }
                                }
                            }
                        }
                    } else if let Some(cls) = crate::exec::props::get_class(obj, &self.heap) {
                        let slot = entry.slot as usize;
                        if entry.is_class == ICKind::CLASS_METHOD
                            && cls.id == entry.id
                            && entry.vtable_ver
                                == (cls.vtable_version.load(Ordering::Relaxed) & 0xFF) as u8
                        {
                            let vtable = unsafe { &*cls.vtable.as_ptr() };
                            let vtable_owners = unsafe { &*cls.vtable_owners.as_ptr() };
                            if slot < vtable.len() {
                                found_method =
                                    Some((vtable[slot].clone(), vtable_owners[slot].clone()));
                                hit_found = true;
                                break 'entries;
                            }
                        } else if entry.is_class == ICKind::CLASS_GETTER
                            && cls.id == entry.id
                            && entry.vtable_ver
                                == (cls.vtable_version.load(Ordering::Relaxed) & 0xFF) as u8
                        {
                            let vtable = unsafe { &*cls.getter_vtable.as_ptr() };
                            if slot < vtable.len() {
                                found_getter = Some(vtable[slot].clone());
                                hit_found = true;
                                break 'entries;
                            }
                        }
                    }
                }

                if !hit_found {
                    self.record_ic_miss_getprop();
                }
            }

            if let Some(v) = found_slot_val {
                self.record_ic_hit_getprop();
                self.stack[base + dest] = v;
                return Ok(false);
            } else if let Some((method, owner)) = found_method {
                self.record_ic_hit_getprop();
                let receiver = self.heap.extract(obj);
                let bound = crate::exec::props::bind_method_to_receiver(receiver, method, owner);
                self.stack[base + dest] = self.heap.intern(bound);
                return Ok(false);
            } else if let Some(getter_val) = found_getter {
                self.record_ic_hit_getprop();
                let getter_nv = self.heap.intern(getter_val);
                return self.call_getter_sync(getter_nv, obj, dest, base, frame_idx);
            }
        }

        if name.as_ref() == varn_core::MemberKey::Length.as_str() {
            if let Some(v) = crate::exec::strings::fast_length(obj, &self.heap) {
                self.stack[base + dest] = v;
                if cs_idx < cache_len && !is_megamorphic {
                    let is_str = obj.is_sso()
                        || matches!(
                            self.heap.get(obj.as_heap_idx()),
                            Some(crate::heap::HeapObj::Str(_))
                        );
                    if let Some(cls) = crate::exec::props::get_class(obj, &self.heap) {
                        let entry = varn_types::chunk::CacheEntry {
                            id: cls.id,
                            slot: 0,
                            is_class: if is_str {
                                ICKind::STR_LENGTH
                            } else {
                                ICKind::ARRAY_LENGTH
                            },
                            vtable_ver: 0,
                        };
                        closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                        closure.feedback.borrow_mut().observe(cs_idx, cls.id);
                    }
                }
                return Ok(false);
            }
        }

        if let Some(getter_val) = crate::exec::props::find_getter(obj, &name, &self.heap) {
            if cs_idx < cache_len && !is_megamorphic {
                if let Some(cls) = crate::exec::props::get_class(obj, &self.heap) {
                    if let Some(&slot) = cls.getter_map.borrow().get(name.as_ref()) {
                        let entry = varn_types::chunk::CacheEntry {
                            id: cls.id,
                            slot: slot as u16,
                            is_class: ICKind::CLASS_GETTER,
                            vtable_ver: (cls.vtable_version.load(Ordering::Relaxed) & 0xFF) as u8,
                        };
                        closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                        closure.feedback.borrow_mut().observe(cs_idx, cls.id);
                    }
                }
            }
            let getter_nv = self.heap.intern(getter_val);
            return self.call_getter_sync(getter_nv, obj, dest, base, frame_idx);
        }

        let val_res = crate::exec::props::get_property(obj, &name, &mut self.heap);
        match val_res {
            Ok(val) => {
                self.stack[base + dest] = val;
            }
            Err(e) => {
                return Err(crate::error::RuntimeError::new(format!("{}", e)));
            }
        }

        if cs_idx < cache_len && !is_megamorphic {
            if obj.is_heap() {
                if let Some(crate::heap::HeapObj::Object(o) | crate::heap::HeapObj::Record(o)) =
                    self.heap.get(obj.as_heap_idx())
                {
                    let guard = o.read();
                    if let Some(&slot) = guard.shape().property_names.get(name.as_ref()) {
                        if slot < guard.slot_count() {
                            let shape_id = guard.shape().id;
                            let entry = varn_types::chunk::CacheEntry {
                                id: shape_id,
                                slot: slot as u16,
                                is_class: ICKind::SHAPE_PROP,
                                vtable_ver: 0,
                            };
                            closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                            closure.feedback.borrow_mut().observe(cs_idx, shape_id);
                            return Ok(false);
                        }
                    }
                }
            }
            if let Some(cls) = crate::exec::props::get_class(obj, &self.heap) {
                if let Some(&slot) = cls.method_map.borrow().get(name.as_ref()) {
                    let entry = varn_types::chunk::CacheEntry {
                        id: cls.id,
                        slot: slot as u16,
                        is_class: ICKind::CLASS_METHOD,
                        vtable_ver: (cls.vtable_version.load(Ordering::Relaxed) & 0xFF) as u8,
                    };
                    closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                    closure.feedback.borrow_mut().observe(cs_idx, cls.id);
                }
            }
        }

        Ok(false)
    }
}
