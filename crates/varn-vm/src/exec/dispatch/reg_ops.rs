use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::frame::{VmClosure, VmClosurePayload};
use crate::heap::HeapObj;
use crate::value::VmValue;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use varn_core::OpCode;
use varn_types::{PoolEntry, Value};

impl ExecCtx {
    pub(super) fn exec_variable_op(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        frame_idx: usize,
        closure: &VmClosure,
        first_reg: usize,
    ) -> VmResult<()> {
        match op {
            OpCode::LoadGlobal => {
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let name_nv = closure.constants[name_idx];
                let name = self
                    .heap
                    .str_val(name_nv)
                    .ok_or_else(|| RuntimeError::new("LoadGlobal: non-string const"))?;
                let val = self.globals.get_by_name(&name).unwrap_or(VmValue::null());
                self.stack[base + first_reg] = val;
            }
            OpCode::StoreGlobal => {
                let src = (code[*ip] >> 8) as usize;
                *ip += 1;
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let name_nv = closure.constants[name_idx];
                let name = self
                    .heap
                    .str_val(name_nv)
                    .ok_or_else(|| RuntimeError::new("StoreGlobal: non-string const"))?;
                let val = self.stack[base + src];
                self.globals.set_by_name(&name, val);
            }
            OpCode::DefineGlobal => {
                let src = (code[*ip] >> 8) as usize;
                *ip += 1;
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let name_nv = closure.constants[name_idx];
                let name = self
                    .heap
                    .str_val(name_nv)
                    .ok_or_else(|| RuntimeError::new("DefineGlobal: non-string const"))?;
                let val = self.stack[base + src];
                self.globals.define(&name, val);
            }
            OpCode::LoadGlobalIdx => {
                let idx = code[*ip] as usize;
                *ip += 1;
                let val = self.globals.get_by_index(idx).unwrap_or(VmValue::null());
                self.stack[base + first_reg] = val;
            }
            OpCode::StoreGlobalIdx => {
                let src = (code[*ip] >> 8) as usize;
                *ip += 1;
                let idx = code[*ip] as usize;
                *ip += 1;
                let val = self.stack[base + src];
                self.globals.set_by_index(idx, val);
            }
            OpCode::DefineGlobalIdx => {
                let src = (code[*ip] >> 8) as usize;
                *ip += 1;
                let idx = code[*ip] as usize;
                *ip += 1;
                let val = self.stack[base + src];
                self.globals.set_by_index(idx, val);
            }
            _ => {}
        }
        self.frames[frame_idx].ip = *ip;
        Ok(())
    }

    pub(super) fn exec_call_reg(
        &mut self,
        callee: VmValue,
        base: usize,
        arg_start: usize,
        arg_count: usize,
        dest: usize,
        frame_idx: usize,
    ) -> VmResult<bool> {
        if callee.is_heap() {
            match self.heap.get(callee.as_heap_idx()) {
                Some(crate::heap::HeapObj::NativeFn(_name, f)) => {
                    let f = *f;
                    self.record_call_native();
                    let result = if arg_count <= 16 {
                        let mut buf = [
                            VmValue::null(),
                            VmValue::null(),
                            VmValue::null(),
                            VmValue::null(),
                            VmValue::null(),
                            VmValue::null(),
                            VmValue::null(),
                            VmValue::null(),
                            VmValue::null(),
                            VmValue::null(),
                            VmValue::null(),
                            VmValue::null(),
                            VmValue::null(),
                            VmValue::null(),
                            VmValue::null(),
                            VmValue::null(),
                        ];
                        for i in 0..arg_count {
                            buf[i] = self.stack[base + arg_start + i];
                        }
                        (f)(self as &mut dyn varn_types::NativeCtx, &buf[..arg_count])
                    } else {
                        let varn_args: Vec<VmValue> = (0..arg_count)
                            .map(|i| self.stack[base + arg_start + i])
                            .collect();
                        (f)(self as &mut dyn varn_types::NativeCtx, &varn_args)
                    }
                    .map_err(|e| RuntimeError::new(e))?;
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

    pub(super) fn exec_call_method_reg(
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

        let receiver_class = if this_val.is_heap() {
            if let Some(crate::heap::HeapObj::Object(o)) = self.heap.get(this_val.as_heap_idx()) {
                o.borrow().class()
            } else {
                None
            }
        } else {
            None
        };
        let cache_len = closure.ic_cache_len();
        if receiver_class.is_some() && cs < cache_len {
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
                                            && !nc.proto.has_rest
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
                let result = if arg_count + 1 <= 16 {
                    let mut buf = [
                        VmValue::null(),
                        VmValue::null(),
                        VmValue::null(),
                        VmValue::null(),
                        VmValue::null(),
                        VmValue::null(),
                        VmValue::null(),
                        VmValue::null(),
                        VmValue::null(),
                        VmValue::null(),
                        VmValue::null(),
                        VmValue::null(),
                        VmValue::null(),
                        VmValue::null(),
                        VmValue::null(),
                        VmValue::null(),
                        VmValue::null(),
                    ];
                    buf[0] = receiver;
                    for i in 0..arg_count {
                        buf[i + 1] = self.stack[base + arg_start + i];
                    }
                    (f)(
                        self as &mut dyn varn_types::NativeCtx,
                        &buf[..arg_count + 1],
                    )
                } else {
                    let mut args = Vec::with_capacity(arg_count + 1);
                    args.push(receiver);
                    for i in 0..arg_count {
                        args.push(self.stack[base + arg_start + i]);
                    }
                    (f)(self as &mut dyn varn_types::NativeCtx, &args)
                }
                .map_err(|e| RuntimeError::new(e))?;
                self.stack[base + dest] = result;
                return Ok(false);
            }

            if let Some((nc, owner_class)) = ic_vm {
                self.record_ic_hit_callmethod();
                self.record_call_vm_fast();
                self.stack.push(VmValue::null());
                self.stack.push(this_val);
                for i in 0..arg_count {
                    let v = self.stack[base + arg_start + i];
                    self.stack.push(v);
                }
                let full_arg_count = arg_count + 1;
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
            if cs < cache_len && method_nv.is_heap() {
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
                                if !nc.proto.is_generator
                                    && !nc.proto.is_async
                                    && !nc.proto.has_rest
                                {
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
            let result = if arg_count + 1 <= 16 {
                let mut buf = [
                    VmValue::null(),
                    VmValue::null(),
                    VmValue::null(),
                    VmValue::null(),
                    VmValue::null(),
                    VmValue::null(),
                    VmValue::null(),
                    VmValue::null(),
                    VmValue::null(),
                    VmValue::null(),
                    VmValue::null(),
                    VmValue::null(),
                    VmValue::null(),
                    VmValue::null(),
                    VmValue::null(),
                    VmValue::null(),
                    VmValue::null(),
                ];
                buf[0] = receiver;
                for i in 0..arg_count {
                    buf[i + 1] = self.stack[base + arg_start + i];
                }
                (f)(
                    self as &mut dyn varn_types::NativeCtx,
                    &buf[..arg_count + 1],
                )
            } else {
                let mut args = Vec::with_capacity(arg_count + 1);
                args.push(receiver);
                for i in 0..arg_count {
                    args.push(self.stack[base + arg_start + i]);
                }
                (f)(self as &mut dyn varn_types::NativeCtx, &args)
            }
            .map_err(|e| RuntimeError::new(e))?;
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
        self.push(method_nv);
        let skip_this = is_bound
            || is_static
            || is_enum_variant
            || is_plain_closure_no_this
            || is_namespace_native;
        if !skip_this {
            self.push(this_val);
        }
        for i in 0..arg_count {
            let v = self.stack[base + arg_start + i];
            self.push(v);
        }

        let effective_count = if skip_this { arg_count } else { arg_count + 1 };
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

    pub(super) fn exec_call_spread_reg(
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

    pub(super) fn exec_get_property_reg(
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

        let cache_len = closure.ic_cache_len();
        let obj_is_heap_object = obj.is_heap()
            && matches!(
                self.heap.get(obj.as_heap_idx()),
                Some(crate::heap::HeapObj::Object(_))
            );
        if obj_is_heap_object && cs_idx < cache_len {
            let mut found_slot_val: Option<VmValue> = None;
            let mut found_method: Option<(varn_types::Value, Option<Rc<varn_types::ClassObj>>)> =
                None;
            let mut found_getter: Option<varn_types::Value> = None;
            let mut hit_found = false;

            {
                let slot_cache = closure.ic_cache.borrow();
                let poly_slot = &slot_cache[cs_idx];

                'entries: for entry in &poly_slot.entries {
                    if entry.id == 0 {
                        continue;
                    }

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
                                        hit_found = true;
                                        break 'entries;
                                    }
                                }
                            }
                        }
                    } else if let Some(cls) = crate::exec::props::get_class(obj, &self.heap) {
                        let slot = entry.slot as usize;
                        if entry.is_class == 2
                            && cls.id == entry.id
                            && entry.vtable_ver
                                == (cls.vtable_version.load(Ordering::Relaxed) & 0xFF) as u8
                        {
                            let vtable = cls.vtable.borrow();
                            let vtable_owners = cls.vtable_owners.borrow();
                            if slot < vtable.len() {
                                found_method =
                                    Some((vtable[slot].clone(), vtable_owners[slot].clone()));
                                hit_found = true;
                                break 'entries;
                            }
                        } else if entry.is_class == 3
                            && cls.id == entry.id
                            && entry.vtable_ver
                                == (cls.vtable_version.load(Ordering::Relaxed) & 0xFF) as u8
                        {
                            let vtable = cls.getter_vtable.borrow();
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

        if let Some(getter_val) = crate::exec::props::find_getter(obj, &name, &self.heap) {
            if cs_idx < cache_len {
                if let Some(cls) = crate::exec::props::get_class(obj, &self.heap) {
                    if let Some(&slot) = cls.getter_map.borrow().get(name.as_ref()) {
                        let entry = varn_types::chunk::CacheEntry {
                            id: cls.id,
                            slot: slot as u16,
                            is_class: 3,
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

        if cs_idx < cache_len {
            if obj.is_heap() {
                if let Some(crate::heap::HeapObj::Object(o)) = self.heap.get(obj.as_heap_idx()) {
                    let guard = o.borrow();
                    if let Some(&slot) = guard.inner.shape.property_names.get(name.as_ref()) {
                        if slot < guard.inner.values.len() {
                            let entry = varn_types::chunk::CacheEntry {
                                id: guard.inner.shape.id,
                                slot: slot as u16,
                                is_class: 1,
                                vtable_ver: 0,
                            };
                            let shape_id = guard.inner.shape.id;
                            drop(guard);
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
                        is_class: 2,
                        vtable_ver: (cls.vtable_version.load(Ordering::Relaxed) & 0xFF) as u8,
                    };
                    closure.ic_cache.borrow_mut()[cs_idx].find_or_insert(entry);
                    closure.feedback.borrow_mut().observe(cs_idx, cls.id);
                }
            }
        }

        Ok(false)
    }

    pub(super) fn exec_set_property_reg(
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

        let cache_len = closure.ic_cache_len();
        let obj_is_heap_object = obj.is_heap()
            && matches!(
                self.heap.get(obj.as_heap_idx()),
                Some(crate::heap::HeapObj::Object(_))
            );
        if obj_is_heap_object && cs_idx < cache_len {
            let mut found_slot: Option<(usize, u8)> = None;
            let mut found_setter: Option<varn_types::Value> = None;
            let mut hit_found = false;

            {
                let slot_cache = closure.ic_cache.borrow();
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
                            let guard = o.borrow();
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
                                let vtable = cls.setter_vtable.borrow();
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
                        let mut guard = o.borrow_mut();
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
            if cs_idx < cache_len {
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
                        if cs_idx < cache_len {
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
                if cs_idx < cache_len {
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

    fn call_setter_sync(
        &mut self,
        setter_nv: VmValue,
        receiver: VmValue,
        value: VmValue,
        _base: usize,
    ) -> VmResult<()> {
        let setter_val = self.heap.extract(setter_nv);
        let receiver_val = self.heap.extract(receiver);
        let value_val = self.heap.extract(value);
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
                    frame.return_reg = None;
                    let required = frame.base + frame.closure.proto.register_count as usize;
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

    pub(super) fn exec_get_fixed_field(&mut self, obj: VmValue, slot: usize) -> VmResult<VmValue> {
        crate::exec::props::get_fixed_field(obj, slot, &mut self.heap)
    }

    pub(super) fn exec_set_fixed_field(
        &mut self,
        obj: VmValue,
        slot: usize,
        val: VmValue,
    ) -> VmResult<()> {
        crate::exec::props::set_fixed_field(obj, slot, val, &mut self.heap)
    }

    pub(super) fn exec_get_super_reg(
        &mut self,
        this_val: VmValue,
        name_idx: usize,
        frame_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<VmValue> {
        let name_nv = closure.constants[name_idx];
        let name = self
            .heap
            .str_val(name_nv)
            .ok_or_else(|| RuntimeError::new("GetSuper: non-string const"))?;

        let cls = self.frames[frame_idx]
            .current_class
            .clone()
            .or_else(|| crate::exec::props::get_class(this_val, &self.heap))
            .ok_or_else(|| RuntimeError::new("GetSuper: 'this' has no class"))?;
        let class_nv = self.heap.intern(varn_types::Value::Class(cls));
        crate::exec::class::op_get_super(class_nv, &name, this_val, &mut self.heap)
    }

    pub(super) fn exec_get_symbol(
        &mut self,
        obj: VmValue,
        sym_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<VmValue> {
        let sym_nv = closure.constants[sym_idx];
        let sym_val = self.heap.extract(sym_nv);
        match sym_val {
            varn_types::Value::Symbol(s) => {
                crate::exec::advanced::get_symbol_property(obj, s, &mut self.heap)
            }
            _ => Err(crate::error::RuntimeError::new(
                "GetSymbol: non-symbol constant",
            )),
        }
    }

    pub(super) fn exec_assert_not_null(&self, v: VmValue) -> VmResult<()> {
        if v.is_null() {
            Err(RuntimeError::new("assertion failed: value is null"))
        } else {
            Ok(())
        }
    }

    pub(super) fn exec_declare_field(
        &mut self,
        obj: VmValue,
        name_idx: usize,
        _frame_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<()> {
        let name_nv = closure.constants[name_idx];
        let name = self
            .heap
            .str_val(name_nv)
            .ok_or_else(|| RuntimeError::new("DeclareField: non-string const"))?;
        crate::exec::class::op_declare_field(obj, &name, &mut self.heap)
    }

    pub(super) fn exec_get_index_nv(&mut self, obj: VmValue, key_nv: VmValue) -> VmResult<VmValue> {
        crate::exec::collections::get_index(obj, key_nv, &mut self.heap)
    }

    pub(super) fn exec_set_index(
        &mut self,
        obj: VmValue,
        idx: VmValue,
        val: VmValue,
    ) -> VmResult<()> {
        crate::exec::collections::set_index(obj, idx, val, &mut self.heap)
    }

    pub(super) fn exec_build_object_with_shape(
        &mut self,
        base: usize,
        start_reg: usize,
        shape_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<VmValue> {
        let keys = match closure.proto.chunk.constants.get(shape_idx) {
            Some(PoolEntry::Shape(k)) => k.clone(),
            _ => {
                return Err(RuntimeError::new(
                    "BuildObjectWithShape: invalid shape const",
                ))
            }
        };
        let count = keys.len();
        let mut vals: Vec<VmValue> = Vec::with_capacity(count);
        for i in 0..count {
            vals.push(self.stack[base + start_reg + i]);
        }

        let old_len = self.stack.len();
        for v in vals {
            self.stack.push(v);
        }
        let result = crate::exec::collections::build_object_with_shape(
            &mut self.stack,
            &keys,
            &mut self.heap,
        );
        Ok(result)
    }

    pub(super) fn exec_object_rest(
        &mut self,
        obj: VmValue,
        skip_keys: &[Rc<str>],
    ) -> VmResult<VmValue> {
        let owned: Vec<String> = skip_keys.iter().map(|s| s.to_string()).collect();
        crate::exec::collections::object_rest(obj, &owned, &mut self.heap)
    }

    pub(super) fn exec_object_keys(&mut self, obj: VmValue) -> VmResult<VmValue> {
        crate::exec::collections::object_keys(obj, &mut self.heap)
    }

    pub(super) fn exec_array_length(&mut self, arr: VmValue) -> VmResult<VmValue> {
        crate::exec::collections::array_length(arr, &self.heap)
    }

    pub(super) fn exec_array_push(&mut self, arr: VmValue, val: VmValue) -> VmResult<()> {
        crate::exec::collections::array_push(arr, val, &mut self.heap)
    }

    pub(super) fn exec_array_pop(&mut self, arr: VmValue) -> VmResult<VmValue> {
        crate::exec::collections::array_pop(arr, &mut self.heap)
    }

    pub(super) fn exec_array_extend(&mut self, arr: VmValue, src: VmValue) -> VmResult<()> {
        crate::exec::collections::array_extend(arr, src, &self.heap)
    }

    pub(super) fn exec_str_length(&mut self, v: VmValue) -> VmResult<VmValue> {
        if v.is_heap() {
            if let Some(crate::heap::HeapObj::Str(s)) = self.heap.get(v.as_heap_idx()) {
                return Ok(VmValue::from_i32(s.chars().count() as i32));
            }
        }
        Ok(VmValue::from_i32(0))
    }

    pub(super) fn exec_str_slice(&mut self, s: VmValue, idx: VmValue) -> VmResult<VmValue> {
        if s.is_heap() {
            if let Some(crate::heap::HeapObj::Str(st)) = self.heap.get(s.as_heap_idx()) {
                let start = idx.as_i32().max(0) as usize;
                let sliced: String = st.chars().skip(start).collect();
                return Ok(self.heap.alloc_str(sliced));
            }
        }
        Ok(self.heap.alloc_str(""))
    }

    pub(super) fn exec_class_op(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        frame_idx: usize,
        closure: &VmClosure,
        first_reg: usize,
    ) -> VmResult<()> {
        match op {
            OpCode::MakeClass => {
                let super_reg = (code[*ip] >> 8) as usize;
                *ip += 1;
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let dest = first_reg;
                let name_nv = closure.constants[name_idx];
                let name = self
                    .heap
                    .str_val(name_nv)
                    .ok_or_else(|| RuntimeError::new("MakeClass: non-string const"))?;
                let cls = crate::exec::class::op_class(&name, &mut self.heap);
                self.stack[base + dest] = cls;
                if super_reg != 0 {
                    let super_nv = self.stack[base + super_reg];
                    crate::exec::class::op_inherit(cls, super_nv, &mut self.heap)?;
                }
            }
            OpCode::Inherit => {
                let w1 = code[*ip];
                *ip += 1;
                let (class_reg, super_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let class_nv = self.stack[base + class_reg];
                let super_nv = self.stack[base + super_reg];
                crate::exec::class::op_inherit(class_nv, super_nv, &mut self.heap)?;
            }
            OpCode::Method => {
                let w1 = code[*ip];
                *ip += 1;
                let key_idx = code[*ip] as usize;
                *ip += 1;
                let (class_reg, fn_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let class_nv = self.stack[base + class_reg];
                let fn_nv = self.stack[base + fn_reg];
                let key_nv = closure.constants[key_idx];
                let key = self
                    .heap
                    .str_val(key_nv)
                    .ok_or_else(|| RuntimeError::new("Method: non-string const"))?;
                crate::exec::class::op_method(class_nv, &key, fn_nv, &mut self.heap)?;
            }
            OpCode::DefineStatic => {
                let w1 = code[*ip];
                *ip += 1;
                let key_idx = code[*ip] as usize;
                *ip += 1;
                let (class_reg, fn_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let class_nv = self.stack[base + class_reg];
                let fn_nv = self.stack[base + fn_reg];
                let key_nv = closure.constants[key_idx];
                let key = self
                    .heap
                    .str_val(key_nv)
                    .ok_or_else(|| RuntimeError::new("DefineStatic: non-string const"))?;
                crate::exec::class::op_define_static(class_nv, &key, fn_nv, &mut self.heap)?;
            }
            OpCode::DefineGetter => {
                let w1 = code[*ip];
                *ip += 1;
                let key_idx = code[*ip] as usize;
                *ip += 1;
                let (class_reg, fn_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let class_nv = self.stack[base + class_reg];
                let fn_nv = self.stack[base + fn_reg];
                let key_nv = closure.constants[key_idx];
                let key = self
                    .heap
                    .str_val(key_nv)
                    .ok_or_else(|| RuntimeError::new("DefineGetter: non-string const"))?;
                crate::exec::class::op_define_getter(class_nv, &key, fn_nv, &mut self.heap)?;
            }
            OpCode::DefineSetter => {
                let w1 = code[*ip];
                *ip += 1;
                let key_idx = code[*ip] as usize;
                *ip += 1;
                let (class_reg, fn_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let class_nv = self.stack[base + class_reg];
                let fn_nv = self.stack[base + fn_reg];
                let key_nv = closure.constants[key_idx];
                let key = self
                    .heap
                    .str_val(key_nv)
                    .ok_or_else(|| RuntimeError::new("DefineSetter: non-string const"))?;
                crate::exec::class::op_define_setter(class_nv, &key, fn_nv, &mut self.heap)?;
            }
            OpCode::DefineStaticGetter => {
                let w1 = code[*ip];
                *ip += 1;
                let key_idx = code[*ip] as usize;
                *ip += 1;
                let (class_reg, fn_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let class_nv = self.stack[base + class_reg];
                let fn_nv = self.stack[base + fn_reg];
                let key_nv = closure.constants[key_idx];
                let key = self
                    .heap
                    .str_val(key_nv)
                    .ok_or_else(|| RuntimeError::new("DefineStaticGetter: non-string const"))?;
                crate::exec::class::op_define_static_getter(class_nv, &key, fn_nv, &mut self.heap)?;
            }
            OpCode::DefineStaticSetter => {
                let w1 = code[*ip];
                *ip += 1;
                let key_idx = code[*ip] as usize;
                *ip += 1;
                let (class_reg, fn_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let class_nv = self.stack[base + class_reg];
                let fn_nv = self.stack[base + fn_reg];
                let key_nv = closure.constants[key_idx];
                let key = self
                    .heap
                    .str_val(key_nv)
                    .ok_or_else(|| RuntimeError::new("DefineStaticSetter: non-string const"))?;
                crate::exec::class::op_define_static_setter(class_nv, &key, fn_nv, &mut self.heap)?;
            }
            OpCode::DeclareField => {
                let w1 = code[*ip];
                *ip += 1;
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let class_reg = (w1 >> 8) as usize;
                let class_nv = self.stack[base + class_reg];
                let key_nv = closure.constants[name_idx];
                let key = self
                    .heap
                    .str_val(key_nv)
                    .ok_or_else(|| RuntimeError::new("DeclareField: non-string const"))?;
                crate::exec::class::op_declare_field(class_nv, &key, &mut self.heap)?;
            }
            OpCode::BindMethod => {
                let w1 = code[*ip];
                *ip += 1;
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let (dest, obj_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let obj_nv = self.stack[base + obj_reg];
                let key_nv = closure.constants[name_idx];
                let key = self
                    .heap
                    .str_val(key_nv)
                    .ok_or_else(|| RuntimeError::new("BindMethod: non-string const"))?;
                let method = crate::exec::props::get_property(obj_nv, &key, &mut self.heap)?;

                let receiver = self.heap.extract(obj_nv);
                let method_val = self.heap.extract(method);
                let bound =
                    varn_types::Value::BoundMethod(Box::new(varn_types::value::BoundMethod {
                        receiver,
                        target: match method_val {
                            varn_types::Value::NativeFn(b) => {
                                varn_types::value::BoundMethodTarget::Native {
                                    func: b.0,
                                    name: b.1,
                                }
                            }
                            varn_types::Value::VmValue(payload) => {
                                varn_types::value::BoundMethodTarget::Vm {
                                    closure: payload,
                                    owner_class: None,
                                }
                            }
                            _ => {
                                return Err(RuntimeError::new("BindMethod: method is not callable"))
                            }
                        },
                    }));
                self.stack[base + dest] = self.heap.intern(bound);
            }
            _ => {}
        }
        self.frames[frame_idx].ip = *ip;
        Ok(())
    }

    pub(super) fn exec_make_enum_variant_reg(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        frame_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<()> {
        let w1 = code[*ip];
        *ip += 1;
        let name_idx = code[*ip] as usize;
        *ip += 1;
        let (dest, tag_reg) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
        let name_nv = closure.constants[name_idx];
        let name = self
            .heap
            .str_val(name_nv)
            .ok_or_else(|| RuntimeError::new("MakeEnumVariant: non-string const"))?;
        let tag = self.stack[base + tag_reg].as_i32() as u8;
        let variant =
            varn_types::Value::EnumVariant(Box::new(varn_types::value::EnumVariantData {
                variant_name: Rc::from(name.as_ref()),
                variant_tag: tag,
                payload: varn_types::Value::Null,
            }));
        self.stack[base + dest] = self.heap.intern(variant);
        self.frames[frame_idx].ip = *ip;
        Ok(())
    }

    pub(super) fn exec_get_enum_tag(&mut self, v: VmValue) -> VmResult<VmValue> {
        let val = self.heap.extract(v);
        match val {
            Value::EnumVariant(ev) => Ok(VmValue::from_i32(ev.variant_tag as i32)),
            _ => Ok(VmValue::from_i32(0)),
        }
    }

    pub(super) fn exec_spawn(&mut self, task_val: VmValue) -> VmResult<VmValue> {
        let task = self.heap.extract(task_val);

        Ok(self.heap.intern(task))
    }

    pub(super) fn exec_module_op_reg(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        frame_idx: usize,
        closure: &VmClosure,
        first_reg: usize,
    ) -> VmResult<()> {
        match op {
            OpCode::Import => {
                self.op_import(code, ip, closure, frame_idx, first_reg)?;
            }
            OpCode::Reexport => {
                self.op_reexport(code, ip, frame_idx, closure)?;
            }
            OpCode::MergeExports => {
                self.op_merge_exports(code, ip, frame_idx, first_reg)?;
            }
            _ => {}
        }
        self.frames[frame_idx].ip = *ip;
        Ok(())
    }

    pub(super) fn exec_invoke_runtime_static_reg(
        &mut self,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        frame_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<()> {
        let w1 = code[*ip];
        *ip += 1;
        let method_idx = code[*ip] as usize;
        *ip += 1;
        let w3 = code[*ip];
        *ip += 1;
        let w4 = code[*ip];
        *ip += 1;
        let dest = (w1 >> 8) as usize;
        let arg_count = (w3 >> 8) as usize;
        let arg_start = (w3 & 0xFF) as usize;
        let end_reg = (w4 >> 8) as usize;
        let flag = (w4 & 0xFF) as u16;

        let name_nv = closure.constants[method_idx];
        let name = self.heap.str_val(name_nv).ok_or_else(|| {
            RuntimeError::new(format!(
                "InvokeRuntimeStatic: const[{}] not a string",
                method_idx
            ))
        })?;

        if arg_count == 2 {
            let s = self.stack[base + arg_start];
            let e = self.stack[base + end_reg];
            self.stack.push(s);
            self.stack.push(e);
        } else {
            for i in 0..arg_count {
                let v = self.stack[base + arg_start + i];
                self.stack.push(v);
            }
        }

        let result = crate::exec::advanced::invoke_runtime_static(
            &name,
            &mut self.stack,
            &mut self.heap,
            flag,
        )?;
        self.stack[base + dest] = result;
        self.frames[frame_idx].ip = *ip;
        Ok(())
    }

    pub(super) fn call_getter_sync(
        &mut self,
        getter_nv: VmValue,
        receiver: VmValue,
        dest: usize,
        base: usize,
        _frame_idx: usize,
    ) -> VmResult<bool> {
        let getter_val = self.heap.extract(getter_nv);
        let receiver_val = self.heap.extract(receiver);
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
                        frame.return_reg = Some(dest as u16);
                        let required = frame.base + frame.closure.proto.register_count as usize;
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
