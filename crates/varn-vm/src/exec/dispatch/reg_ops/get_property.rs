use crate::closure::VmClosure;
use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use varn_types::chunk::ICKind;

pub(crate) enum PropRead {
    Value(VmValue),
    Getter(VmValue),
}

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
        match self.read_property(obj, name_idx, cs_idx, closure)? {
            PropRead::Value(v) => {
                self.stack.unbox_into_reg(base, dest, v)?;
                Ok(false)
            }
            PropRead::Getter(g) => self.call_getter_sync(g, obj, dest, base, frame_idx),
        }
    }

    pub(crate) fn get_property_value(
        &mut self,
        obj: VmValue,
        name_idx: usize,
        cs_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<VmValue> {
        match self.read_property(obj, name_idx, cs_idx, closure)? {
            PropRead::Value(v) => Ok(v),
            PropRead::Getter(g) => {
                if let Some((f, _)) = self.heap.native_of(g) {
                    return self.invoke_native(f, &[obj]).map_err(RuntimeError::from);
                }
                self.invoke(g, &[obj])
            }
        }
    }

    fn read_property(
        &mut self,
        obj: VmValue,
        name_idx: usize,
        cs_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<PropRead> {
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
            && (self.heap.instance(obj.as_heap()).is_some()
                || matches!(
                    self.heap.get(obj.as_heap()),
                    Some(crate::heap::HeapObj::Object(_) | crate::heap::HeapObj::Record(_))
                ));
        if obj_is_heap_object && cs_idx < cache_len && !is_megamorphic {
            let mut found_slot_val: Option<VmValue> = None;
            let mut found_method: Option<(VmValue, Option<Rc<varn_types::ClassObj>>)> = None;
            let mut found_getter: Option<VmValue> = None;
            let mut hit_found = false;

            {
                let slot_cache = unsafe { &*closure.ic_cache.as_ptr() };
                let poly_slot = &slot_cache[cs_idx];

                'entries: for entry in &poly_slot.entries {
                    if entry.id == 0 {
                        continue;
                    }

                    if entry.is_class == ICKind::INSTANCE_FIELD {
                        if obj.is_heap() {
                            if let Some(inst) = self.heap.instance(obj.as_heap()) {
                                if inst.class_id == entry.id {
                                    if let Some(v) = inst.field_at(entry.slot as usize) {
                                        found_slot_val = Some(v);
                                        hit_found = true;
                                        break 'entries;
                                    }
                                }
                            }
                        }
                    } else if entry.is_class == ICKind::SHAPE_PROP {
                        if obj.is_heap() {
                            if let Some(
                                crate::heap::HeapObj::Object(o) | crate::heap::HeapObj::Record(o),
                            ) = self.heap.get(obj.as_heap())
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
                                found_method = Some((vtable[slot], vtable_owners[slot].clone()));
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
                                found_getter = Some(vtable[slot]);
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
                return Ok(PropRead::Value(v));
            } else if let Some((method, owner)) = found_method {
                self.record_ic_hit_getprop();
                let bound_nv =
                    crate::exec::props::bind_method_to_receiver(&mut self.heap, obj, method, owner);
                return Ok(PropRead::Value(bound_nv));
            } else if let Some(getter_val) = found_getter {
                self.record_ic_hit_getprop();
                return Ok(PropRead::Getter(getter_val));
            }
        }

        if name.as_ref() == varn_core::MemberKey::Length.as_str() {
            if let Some(v) = crate::exec::strings::fast_length(obj, &self.heap) {
                if cs_idx < cache_len && !is_megamorphic {
                    let is_str = obj.is_sso()
                        || matches!(
                            self.heap.get(obj.as_heap()),
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
                            class: None,
                        };
                        closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                        closure.feedback.borrow_mut().observe(cs_idx, cls.id);
                    }
                }
                return Ok(PropRead::Value(v));
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
                            class: Some(cls.clone()),
                        };
                        closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                        closure.feedback.borrow_mut().observe(cs_idx, cls.id);
                    }
                }
            }
            return Ok(PropRead::Getter(getter_val));
        }

        let val = match crate::exec::props::get_property(obj, &name, &mut self.heap) {
            Ok(val) => val,
            Err(e) => {
                return Err(crate::error::RuntimeError::new(format!("{}", e)));
            }
        };

        if cs_idx < cache_len && !is_megamorphic {
            if obj.is_heap() {
                if let Some(inst) = self.heap.instance(obj.as_heap()) {
                    if let Some(cls) = varn_types::ClassObj::find_by_id(inst.class_id) {
                        let root = cls.root_shape.borrow();
                        if let Some(&slot) = root.property_names.get(name.as_ref()) {
                            let entry = varn_types::chunk::CacheEntry {
                                id: inst.class_id,
                                slot: slot as u16,
                                is_class: ICKind::INSTANCE_FIELD,
                                vtable_ver: 0,
                                class: None,
                            };
                            closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                            closure.feedback.borrow_mut().observe(cs_idx, inst.class_id);
                            return Ok(PropRead::Value(val));
                        }
                    }
                }
                match self.heap.get(obj.as_heap()) {
                    Some(crate::heap::HeapObj::Object(o) | crate::heap::HeapObj::Record(o)) => {
                        let guard = o.read();
                        if let Some(&slot) = guard.shape().property_names.get(name.as_ref()) {
                            if slot < guard.slot_count() {
                                let shape_id = guard.shape().id;
                                let entry = varn_types::chunk::CacheEntry {
                                    id: shape_id,
                                    slot: slot as u16,
                                    is_class: ICKind::SHAPE_PROP,
                                    vtable_ver: 0,
                                    class: None,
                                };
                                closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                                closure.feedback.borrow_mut().observe(cs_idx, shape_id);
                                return Ok(PropRead::Value(val));
                            }
                        }
                    }
                    _ => {}
                }
            }
            if let Some(cls) = crate::exec::props::get_class(obj, &self.heap) {
                if let Some(&slot) = cls.method_map.borrow().get(name.as_ref()) {
                    let entry = varn_types::chunk::CacheEntry {
                        id: cls.id,
                        slot: slot as u16,
                        is_class: ICKind::CLASS_METHOD,
                        vtable_ver: (cls.vtable_version.load(Ordering::Relaxed) & 0xFF) as u8,
                        class: Some(cls.clone()),
                    };
                    closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                    closure.feedback.borrow_mut().observe(cs_idx, cls.id);
                }
            }
        }

        Ok(PropRead::Value(val))
    }
}
