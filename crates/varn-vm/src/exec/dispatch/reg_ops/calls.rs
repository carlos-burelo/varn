use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_types::{Value, VmArray};

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
        // Receiver pendiente para el camino lento (bound-method que no entró
        // en vía rápida): ocupa staging[0] al preparar.
        let mut pending_receiver: Option<VmValue> = None;
        if callee.is_heap() {
            let heap_idx = callee.as_heap_idx();
            if let Some(crate::heap::HeapObj::VmClosure(nc)) = self.heap.get(heap_idx) {
                if !nc.proto.is_generator && !nc.proto.is_async {
                    let arity = nc.proto.arity;
                    if !nc.proto.has_rest && arg_count <= arity {
                        let nc = nc.clone();
                        if self.hotspot_counters.is_some() {
                            let fn_name = nc.proto.name.as_deref().unwrap_or("<anon>");
                            let is_jit = nc.jit_fn().is_some();
                            self.record_hotspot_fn(fn_name, is_jit);
                        }
                        if self.frames.len() >= 10000 {
                            return Err(crate::error::RuntimeError::new(
                                "stack overflow: call depth exceeded 10000",
                            ));
                        }
                        // La ventana ya trae [pad, args...] en el frame
                        // llamante: se mueve directo a los registros del
                        // callee (conversión por clase, sin boxeo intermedio
                        // cuando las clases coinciden).
                        let alloc = self.stack.push_frame(&nc.proto);
                        for i in 0..arg_count {
                            if let Err(e) =
                                self.stack.mov_cross(alloc, i, base, arg_start + i)
                            {
                                self.stack.pop_frame();
                                return Err(e);
                            }
                        }
                        let mut frame = crate::frame::CallFrame::new_owned(nc, alloc);
                        frame.return_reg = dest as u16;
                        self.record_call_vm_fast();
                        self.frames.push(frame);
                        return Ok(true);
                    }
                }
            }

            let mut bound_method = None;
            if let Some(crate::heap::HeapObj::BoundMethod(bm)) = self.heap.get(heap_idx) {
                bound_method = Some((**bm).clone());
            }

            if let Some(bm) = bound_method {
                let receiver = self.heap.intern(bm.receiver.clone());
                match &bm.target {
                    varn_types::value::BoundMethodTarget::Native { func, name, .. } => {
                        let f = *func;
                        self.record_call_native(f, Some(name));
                        let args = self.stack.box_range(base, arg_start, arg_count);
                        let mut args = args;
                        if !args.is_empty() {
                            args[0] = receiver;
                        } else {
                            args.push(receiver);
                        }
                        let result = self.invoke_native(f, &args).map_err(crate::error::RuntimeError::new)?;
                        self.stack.unbox_into_reg(base, dest, result)?;
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
                                if self.frames.len() >= 10000 {
                                    return Err(crate::error::RuntimeError::new(
                                        "stack overflow: call depth exceeded 10000",
                                    ));
                                }
                                // Ventana: [receiver?, args...] → regs
                                // [r0=receiver, r1..]. Con placeholder, el
                                // receiver lo sustituye; sin él va delante.
                                let alloc = self.stack.push_frame(&nc.proto);
                                let mut dst = 0usize;
                                self.stack.unbox_into_reg(alloc, dst, receiver)?;
                                dst += 1;
                                let first_arg = if arg_count == arity { 1 } else { 0 };
                                for i in first_arg..arg_count {
                                    if let Err(e) =
                                        self.stack.mov_cross(alloc, dst, base, arg_start + i)
                                    {
                                        self.stack.pop_frame();
                                        return Err(e);
                                    }
                                    dst += 1;
                                }
                                let mut frame = crate::frame::CallFrame::new_owned(nc, alloc);
                                frame.return_reg = dest as u16;
                                frame.current_class = owner;
                                self.record_call_vm_fast();
                                self.frames.push(frame);
                                return Ok(true);
                            }
                        }
                    }
                }

                // El receiver se estampa en staging[0] para el camino lento:
                // el código anterior lo hacía siempre (con placeholder lo
                // sustituía; sin él ocupaba el primer slot pusheado) y
                // `prepare_call` lo re-aplica de forma idempotente.
                pending_receiver = Some(receiver);
            } else {
                match self.heap.get(callee.as_heap_idx()) {
                    Some(crate::heap::HeapObj::NativeFn(f, name)) => {
                        let f = *f;
                        let name_str = *name;
                        self.record_call_native(f, Some(name_str));
                        let args = self.stack.box_range(base, arg_start, arg_count);
                        let slice = if args.len() > 1 { &args[1..] } else { &[] };
                        let result = self
                            .invoke_native(f, slice)
                            .map_err(crate::error::RuntimeError::new)?;
                        self.stack.unbox_into_reg(base, dest, result)?;
                        return Ok(false);
                    }
                    Some(crate::heap::HeapObj::VmClosure(nc))
                        if !nc.proto.is_generator && !nc.proto.is_async =>
                    {
                        let arity = nc.proto.arity;

                        if !nc.proto.has_rest && arg_count <= arity {
                            let nc = nc.clone();
                            if self.hotspot_counters.is_some() {
                                let fn_name = nc.proto.name.as_deref().unwrap_or("<anon>");
                                let is_jit = nc.jit_fn().is_some();
                                self.record_hotspot_fn(fn_name, is_jit);
                            }
                            if self.frames.len() >= 10000 {
                                return Err(crate::error::RuntimeError::new(
                                    "stack overflow: call depth exceeded 10000",
                                ));
                            }
                            let alloc = self.stack.push_frame(&nc.proto);
                            for i in 0..arg_count {
                                if let Err(e) =
                                    self.stack.mov_cross(alloc, i, base, arg_start + i)
                                {
                                    self.stack.pop_frame();
                                    return Err(e);
                                }
                            }
                            let mut frame = crate::frame::CallFrame::new_owned(nc, alloc);
                            frame.return_reg = dest as u16;
                            self.record_call_vm_fast();
                            self.frames.push(frame);
                            return Ok(true);
                        } else if nc.proto.has_rest && arg_count <= arity {
                            let nc = nc.clone();
                            let fn_name2 = nc.proto.name.as_deref().unwrap_or("<anon>").to_owned();
                            let is_jit2 = nc.jit_fn().is_some();
                            self.record_hotspot_fn(&fn_name2, is_jit2);
                            if self.frames.len() >= 10000 {
                                return Err(crate::error::RuntimeError::new(
                                    "stack overflow: call depth exceeded 10000",
                                ));
                            }
                            let alloc = self.stack.push_frame(&nc.proto);
                            let rest_idx = arity.saturating_sub(1);
                            let regular_count = arg_count.min(rest_idx);
                            let mut failed: Option<crate::error::RuntimeError> = None;
                            for i in 0..regular_count {
                                if let Err(e) =
                                    self.stack.mov_cross(alloc, i, base, arg_start + i)
                                {
                                    failed = Some(e);
                                    break;
                                }
                            }
                            if failed.is_none() {
                                for i in regular_count..rest_idx {
                                    // `null` de relleno (igual que antes).
                                    if let Err(e) = self.stack.unbox_into_reg(
                                        alloc,
                                        i,
                                        VmValue::null(),
                                    ) {
                                        failed = Some(e);
                                        break;
                                    }
                                }
                            }
                            if failed.is_none() {
                                let rest_items: Vec<VmValue> = if arg_count > rest_idx {
                                    (rest_idx..arg_count)
                                        .map(|i| self.stack.box_reg(base, arg_start + i))
                                        .collect()
                                } else {
                                    vec![]
                                };
                                let rest_nv = VmValue::from_heap_idx(self.heap.alloc(
                                    crate::heap::HeapObj::Array(VmArray::new(rest_items)),
                                ));
                                if let Err(e) =
                                    self.stack.unbox_into_reg(alloc, rest_idx, rest_nv)
                                {
                                    failed = Some(e);
                                }
                            }
                            if let Some(e) = failed {
                                self.stack.pop_frame();
                                return Err(e);
                            }
                            let mut frame = crate::frame::CallFrame::new_owned(nc, alloc);
                            frame.return_reg = dest as u16;
                            self.record_call_vm_fast();
                            self.frames.push(frame);
                            return Ok(true);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Camino lento: la ventana se prepara en staging y `prepare_call` la
        // adopta (frames), la drena (nativas/generadores/async) o la empaqueta.
        self.stage.clear();
        for i in 0..arg_count {
            self.stage.push(self.stack.box_reg(base, arg_start + i));
        }
        if let Some(recv) = pending_receiver {
            if self.stage.is_empty() {
                self.stage.push(recv);
            } else {
                self.stage[0] = recv;
            }
        }

        let prepared = self.prepare_call(callee, arg_count)?;
        self.dispatch_prepared_call(prepared)?;

        if self.frames.len() > frame_idx + 1 {
            self.frames.last_mut().unwrap().return_reg = dest as u16;
            return Ok(true);
        }

        let result = self.stage_pop();
        self.stack.unbox_into_reg(base, dest, result)?;
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

        // Vía lenta: el callee es el propio closure. El código anterior lo
        // leía de `stack[base-1]` (el slot bajo el frame), que no contiene el
        // callee en ningún protocolo de staging: con frames del camino rápido
        // es el último registro del llamante y con frames lentos el primer
        // slot pusheado. La vía rápida (la única que los tests ejercitan) ni
        // lo miraba. Aquí se materializa del `Rc` retenido (o null limpio si
        // el frame es prestado, que `prepare_call` convierte en "not callable"
        // en vez de basura reinterpretada).
        let callee = match &parent_frame._owned_closure {
            Some(rc) => self.heap.alloc_vm_closure(rc.clone()),
            None => VmValue::null(),
        };

        if !closure_ref.proto.is_generator && !closure_ref.proto.is_async {
            let arity = closure_ref.proto.arity;

            if !closure_ref.proto.has_rest && arg_count <= arity {
                if self.hotspot_counters.is_some() {
                    let fn_name = closure_ref.proto.name.as_deref().unwrap_or("<anon>");
                    let is_jit = closure_ref.jit_fn().is_some();
                    self.record_hotspot_fn(fn_name, is_jit);
                }
                if self.frames.len() >= 10000 {
                    return Err(crate::error::RuntimeError::new(
                        "stack overflow: call depth exceeded 10000",
                    ));
                }
                let owned = self.frames[frame_idx]._owned_closure.clone();
                let alloc = self.stack.push_frame(&closure_ref.proto);
                for i in 0..arg_count {
                    if let Err(e) = self.stack.mov_cross(alloc, i, base, arg_start + i) {
                        self.stack.pop_frame();
                        return Err(e);
                    }
                }
                let mut frame = crate::frame::CallFrame::new(closure_ref, alloc);
                frame._owned_closure = owned;
                frame.return_reg = dest as u16;
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
                if self.frames.len() >= 10000 {
                    return Err(crate::error::RuntimeError::new(
                        "stack overflow: call depth exceeded 10000",
                    ));
                }
                let owned = self.frames[frame_idx]._owned_closure.clone();
                let alloc = self.stack.push_frame(&closure_ref.proto);
                let rest_idx = arity.saturating_sub(1);
                let regular_count = arg_count.min(rest_idx);
                let mut failed: Option<crate::error::RuntimeError> = None;
                for i in 0..regular_count {
                    if let Err(e) = self.stack.mov_cross(alloc, i, base, arg_start + i) {
                        failed = Some(e);
                        break;
                    }
                }
                if failed.is_none() {
                    for i in regular_count..rest_idx {
                        if let Err(e) =
                            self.stack.unbox_into_reg(alloc, i, VmValue::null())
                        {
                            failed = Some(e);
                            break;
                        }
                    }
                }
                if failed.is_none() {
                    let rest_items: Vec<VmValue> = if arg_count > rest_idx {
                        (rest_idx..arg_count)
                            .map(|i| self.stack.box_reg(base, arg_start + i))
                            .collect()
                    } else {
                        vec![]
                    };
                    let rest_nv = VmValue::from_heap_idx(self.heap.alloc(
                        crate::heap::HeapObj::Array(VmArray::new(rest_items)),
                    ));
                    if let Err(e) = self.stack.unbox_into_reg(alloc, rest_idx, rest_nv) {
                        failed = Some(e);
                    }
                }
                if let Some(e) = failed {
                    self.stack.pop_frame();
                    return Err(e);
                }
                let mut frame = crate::frame::CallFrame::new(closure_ref, alloc);
                frame._owned_closure = owned;
                frame.return_reg = dest as u16;
                self.record_call_vm_fast();
                self.frames.push(frame);
                return Ok(true);
            }
        }

        self.stage.clear();
        self.stage.push(callee);
        for i in 0..arg_count {
            self.stage.push(self.stack.box_reg(base, arg_start + i));
        }
        let prepared = self.prepare_call(callee, arg_count)?;
        self.dispatch_prepared_call(prepared)?;
        if self.frames.len() > frame_idx + 1 {
            self.frames.last_mut().unwrap().return_reg = dest as u16;
            return Ok(true);
        }
        let result = self.stage_pop();
        self.stack.unbox_into_reg(base, dest, result)?;
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
            let nv = self.stack.box_reg(base, arg_start + i);
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
        self.stage.clear();
        self.stage.push(callee);
        for nv in expanded {
            self.stage.push(nv);
        }
        let prepared = self.prepare_call(callee, flat_count)?;
        self.dispatch_prepared_call(prepared)?;

        if self.frames.len() > frame_idx + 1 {
            self.frames.last_mut().unwrap().return_reg = dest as u16;
            return Ok(true);
        }

        let result = self.stage_pop();
        self.stack.unbox_into_reg(base, dest, result)?;
        Ok(false)
    }
}
