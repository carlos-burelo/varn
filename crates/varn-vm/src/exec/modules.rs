use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use std::rc::Rc;
use varn_core::ModuleId;
use varn_types::Value;

pub fn resolve_specifier_to_id(specifier: &str, from: &ModuleId) -> VmResult<ModuleId> {
    varn_modules::resolver::ModuleResolver::new()
        .resolve(specifier, from)
        .map_err(RuntimeError::new)
}

pub fn resolve_specifier_from_path(specifier: &str, source_file: &str) -> VmResult<ModuleId> {
    let from = if source_file.is_empty() {
        ModuleId::local_str(".")
    } else {
        ModuleId::local_str(source_file)
    };
    resolve_specifier_to_id(specifier, &from)
}

pub fn merge_exports(exports_nv: VmValue, src_nv: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if !exports_nv.is_heap() {
        return Ok(src_nv);
    }
    if !src_nv.is_heap() {
        return Ok(exports_nv);
    }
    let src_val = heap.extract(src_nv);
    let dst_val = heap.extract(exports_nv);

    if let (Value::Object(src_obj), Value::Object(dst_obj)) = (src_val, dst_val) {
        let src_fields: Vec<(Rc<str>, VmValue)> = src_obj
            .borrow()
            .inner
            .iter()
            .map(|(k, nv)| (k.clone(), nv))
            .collect();
        let mut dst_guard = dst_obj.borrow_mut();
        for (name, src_nv) in src_fields {
            let src_val = heap.extract(src_nv);
            let dst_nv = dst_guard.get_field_nv(&name);
            let dst_val = dst_nv.map(|nv| heap.extract(nv));
            match (&dst_val, &src_val) {
                (Some(Value::Object(dst_child)), Value::Object(src_child)) => {
                    let d_nv = heap.intern(Value::Object(dst_child.clone()));
                    let s_nv = heap.intern(Value::Object(src_child.clone()));
                    merge_exports(d_nv, s_nv, heap)?;
                }
                (Some(Value::Class(dst_cls)), Value::Object(src_obj)) => {
                    let src_static_fields: Vec<(Rc<str>, VmValue)> = src_obj
                        .borrow()
                        .inner
                        .iter()
                        .map(|(k, nv)| (k.clone(), nv))
                        .collect();
                    for (sname, snv) in src_static_fields {
                        if !dst_cls.statics.borrow().contains_key(&sname) {
                            dst_cls
                                .statics
                                .borrow_mut()
                                .insert(sname, heap.extract(snv));
                        }
                    }
                }
                (Some(Value::Object(dst_obj)), Value::Class(src_cls)) => {
                    let src_statics = src_cls.statics.borrow().clone();
                    for (sname, sval) in src_statics {
                        if dst_obj.borrow().get_field_nv(&sname).is_none() {
                            let snv = heap.intern(sval);
                            dst_obj.borrow_mut().set_field_nv(sname, snv);
                        }
                    }
                    dst_guard.set_field_nv(name.clone(), heap.intern(src_val));
                }
                (Some(Value::Class(dst_cls)), Value::Class(src_cls)) => {
                    let src_statics = src_cls.statics.borrow().clone();
                    for (sname, sval) in src_statics {
                        if !dst_cls.statics.borrow().contains_key(&sname) {
                            dst_cls.statics.borrow_mut().insert(sname, sval);
                        }
                    }

                    let src_methods = src_cls.method_map.borrow().clone();
                    let src_vtable = src_cls.vtable.borrow();
                    for (mname, &idx) in &src_methods {
                        dst_cls.add_method_with_owner(
                            mname.clone(),
                            src_vtable[idx].clone(),
                            src_cls.vtable_owners.borrow()[idx].clone(),
                        );
                    }

                    let src_getters = src_cls.getter_map.borrow().clone();
                    for (gname, idx) in src_getters {
                        dst_cls.add_getter_with_owner(
                            gname,
                            src_cls.getter_vtable.borrow()[idx].clone(),
                            src_cls.getter_vtable_owners.borrow()[idx].clone(),
                        );
                    }
                    let src_setters = src_cls.setter_map.borrow().clone();
                    for (sname, idx) in src_setters {
                        dst_cls.add_setter_with_owner(
                            sname,
                            src_cls.setter_vtable.borrow()[idx].clone(),
                            src_cls.setter_vtable_owners.borrow()[idx].clone(),
                        );
                    }

                    let src_sgetters = src_cls.static_getter_map.borrow().clone();
                    for (gname, closure) in src_sgetters {
                        dst_cls.add_static_getter(gname, closure);
                    }
                    let src_ssetters = src_cls.static_setter_map.borrow().clone();
                    for (sname, closure) in src_ssetters {
                        dst_cls.add_static_setter(sname, closure);
                    }
                }
                _ => {
                    let mut should_overwrite = true;
                    if let Some(existing) = &dst_val {
                        if matches!(existing, Value::NativeFn(_))
                            || matches!(existing, Value::BoundMethod(_))
                        {
                            if matches!(src_val, Value::VmValue(_))
                                || matches!(src_val, Value::Null)
                            {
                                should_overwrite = false;
                            }
                        }
                    }
                    if should_overwrite {
                        dst_guard.set_field_nv(name.clone(), src_nv);
                    }
                }
            }
        }
        return Ok(exports_nv);
    }
    Ok(exports_nv)
}

pub fn reexport(exports_nv: VmValue, name: &str, val_nv: VmValue, heap: &mut Heap) -> VmResult<()> {
    if exports_nv.is_heap() {
        let o = match heap.get(exports_nv.as_heap_idx()) {
            Some(HeapObj::Object(o)) => o.clone(),
            _ => return Err(RuntimeError::new("OpReexport: exports is not an object")),
        };

        let v = heap.extract(val_nv);
        let existing_nv = o.borrow().get_field_nv(name);
        let existing = existing_nv.map(|nv| heap.extract(nv));

        match (&existing, &v) {
            (Some(Value::Object(dst_child)), Value::Object(src_child)) => {
                let d_nv = heap.intern(Value::Object(dst_child.clone()));
                let s_nv = heap.intern(Value::Object(src_child.clone()));
                merge_exports(d_nv, s_nv, heap)?;
            }
            (Some(Value::Class(dst_cls)), Value::Object(src_obj)) => {
                let src_static_fields: Vec<(Rc<str>, VmValue)> = src_obj
                    .borrow()
                    .inner
                    .iter()
                    .map(|(k, nv)| (k.clone(), nv))
                    .collect();
                for (sname, snv) in src_static_fields {
                    if !dst_cls.statics.borrow().contains_key(&sname) {
                        dst_cls
                            .statics
                            .borrow_mut()
                            .insert(sname, heap.extract(snv));
                    }
                }
            }
            (Some(Value::Class(dst_cls)), Value::Class(src_cls)) => {
                let src_statics = src_cls.statics.borrow().clone();
                for (sname, sval) in src_statics {
                    if !dst_cls.statics.borrow().contains_key(&sname) {
                        dst_cls.statics.borrow_mut().insert(sname, sval);
                    }
                }
            }
            (Some(Value::Object(dst_obj)), Value::Class(src_cls)) => {
                let src_statics = src_cls.statics.borrow().clone();
                for (sname, sval) in src_statics {
                    if dst_obj.borrow().get_field_nv(&sname).is_none() {
                        let snv = heap.intern(sval);
                        dst_obj.borrow_mut().set_field_nv(sname, snv);
                    }
                }
                o.borrow_mut().set_field_nv(Rc::from(name), val_nv);
            }
            _ => {
                o.borrow_mut().set_field_nv(Rc::from(name), val_nv);
            }
        }
        return Ok(());
    }
    Err(RuntimeError::new("OpReexport: exports is not an object"))
}

use varn_modules::resolver::normalize_path_string;
