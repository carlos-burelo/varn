use super::*;

#[inline(always)]
pub(crate) fn try_prepare_call_fast(
    callee_nv: VmValue,
    arg_count: usize,
    staging: &[VmValue],
    heap: &Heap,
    store: &mut FrameStore,
) -> Option<(PreparedCall, bool)> {
    if !callee_nv.is_heap() {
        return None;
    }

    match heap.get(callee_nv.as_heap_idx())? {
        HeapObj::VmClosure(nc) => {
            if !nc.proto.is_generator && !nc.proto.is_async && !nc.proto.has_rest {
                let frame = materialize_frame(store, nc, stage_window(staging, arg_count)).ok()?;
                return Some((PreparedCall::Frame(frame), false));
            }
            None
        }
        // A bound method is the hot shape for every stdlib method call
        // (`arr.push`, `Map.get`, …). Dropping it to the slow `prepare_call`
        // turns 21k immediate native returns into 10k frame pushes — measured
        // 3.8x on tests/main.vn — so both targets keep their fast path. The
        // receiver fills the staged callee slot (`stack[frame.base]`) in
        // `ExecCtx::prepare_call`, which is why the flag comes back `true`.
        // Only the NATIVE target. It is the hot shape for every stdlib method
        // call (`arr.push`, `Map.get`, …) and returns immediately with no frame
        // — dropping it to the slow `prepare_call` turned 21k immediate native
        // returns into 10k frame pushes, measured 3.8x on tests/main.vn.
        //
        // The VM target deliberately stays slow: staging a `CallFrame` here
        // skips the receiver bookkeeping that `super.method()` needs and fails
        // tests/48-opt-unsupported-phase1.vn with "super write via method".
        //
        // The receiver fills the staged callee slot in `ExecCtx::prepare_call`,
        // which is why the flag comes back `true`.
        HeapObj::BoundMethod(bm) => match &bm.target {
            BoundMethodTarget::Native { func, .. } => {
                Some((PreparedCall::NativeImmediate(*func, arg_count), true))
            }
            BoundMethodTarget::Vm { .. } => None,
        },
        HeapObj::NativeFn(f, _name) => {
            Some((PreparedCall::RawNativeImmediate(*f, arg_count), false))
        }
        _ => None,
    }
}

/// Take the generator's arguments off the stack and package its closure, so
/// the caller's context can build the generator's own context by forking
/// itself. Shared by the bare-closure and bound-method paths — writing it
/// twice is what let the empty-globals bug live in both.
fn describe_generator(
    nc: &Rc<VmClosure>,
    arg_count: usize,
    current_class: Option<Rc<varn_types::ClassObj>>,
    staging: &mut Vec<VmValue>,
    heap: &mut Heap,
    settings: crate::settings::ExecSettings,
    store: &FrameStore,
) -> PreparedCall {
    let args_start = staging.len() - arg_count;
    let args: Vec<VmValue> = staging.drain(args_start..).collect();
    let constants = resolve_constants(&nc.proto, heap);
    // Leer DESPUÉS del drain con el store (los upvalues abiertos apuntan a
    // slots del frame, no a staging).
    let upvalues = nc
        .upvalues
        .iter()
        .map(|uv| VmUpvalue::closed(uv.read(store)))
        .collect();
    let mut gen_closure =
        VmClosure::with_upvalues(nc.proto.clone(), upvalues, Rc::new(constants), settings);
    gen_closure.module_base = nc.module_base;
    PreparedCall::Generator {
        closure: Rc::new(gen_closure),
        args,
        current_class,
    }
}

pub(crate) fn prepare_call(
    callee_nv: VmValue,
    arg_count: usize,
    staging: &mut Vec<VmValue>,
    heap: &mut Heap,
    settings: crate::settings::ExecSettings,
    store: &mut FrameStore,
) -> VmResult<PreparedCall> {
    let mut arg_count = arg_count;

    if callee_nv.is_heap() {
        match heap
            .get(callee_nv.as_heap_idx())
            .expect("invalid heap index")
        {
            HeapObj::VmClosure(nc) => {
                let nc = nc.clone();
                bundle_rest_args(&nc.proto, &mut arg_count, staging, heap);
                if nc.proto.is_generator {
                    return Ok(describe_generator(
                        &nc, arg_count, None, staging, heap, settings, store,
                    ));
                }
                if nc.proto.is_async {
                    let args_start = staging.len().saturating_sub(arg_count);
                    let upvalues: Vec<VmValue> =
                        nc.upvalues.iter().map(|uv| uv.read(store)).collect();
                    let task = crate::task::new_lazy(
                        heap,
                        nc.proto.clone(),
                        upvalues,
                        nc.module_base,
                        staging.drain(args_start..),
                        None,
                    );
                    return Ok(PreparedCall::PushValue(task));
                }
                let window: Vec<VmValue> = stage_window(staging, arg_count).to_vec();
                let _ = arg_count;
                return Ok(PreparedCall::Frame(materialize_frame(store, &nc, &window)?));
            }
            HeapObj::NativeFn(f, _name) => {
                let func = *f;
                return Ok(PreparedCall::RawNativeImmediate(func, arg_count));
            }
            HeapObj::BoundMethod(bm) => {
                let bm = bm.clone();
                match bm.target {
                    BoundMethodTarget::Native { func, .. } => {
                        let recv_nv = bm.receiver;
                        let mut final_count = arg_count;
                        // El placeholder de callee es el PRIMER valor de la
                        // ventana (los últimos `arg_count` de staging), no
                        // `staging[0]`: los caminos de método anteponen el
                        // `method_nv` fuera de la ventana.
                        if staging.is_empty() {
                            staging.push(recv_nv);
                            final_count = 1;
                        } else {
                            let start = staging.len().saturating_sub(arg_count);
                            staging[start] = recv_nv;
                        }
                        return Ok(PreparedCall::NativeImmediate(func, final_count));
                    }
                    BoundMethodTarget::Vm {
                        closure,
                        owner_class,
                    } => {
                        let recv_nv = bm.receiver;
                        let nc = if let Some(rc) = heap.closure_of(closure) {
                            rc.clone()
                        } else {
                            return Err(RuntimeError::new(
                                "BoundMethod(Vm): invalid closure payload",
                            ));
                        };
                        // `arity` ya cuenta el registro 0 — el slot de callee
                        // que el llamante prepara como placeholder null — más
                        // los params declarados: el receiver RELLENA ese slot
                        // en staging[0] en vez de desplazar.
                        let mut full_arg_count = arg_count;
                        if staging.is_empty() {
                            staging.push(recv_nv);
                            full_arg_count = 1;
                        } else {
                            let start = staging.len().saturating_sub(full_arg_count);
                            staging[start] = recv_nv;
                        }
                        if nc.proto.is_generator {
                            return Ok(describe_generator(
                                &nc,
                                full_arg_count,
                                owner_class,
                                staging,
                                heap,
                                settings,
                                store,
                            ));
                        }
                        if nc.proto.is_async {
                            let args_start = staging.len().saturating_sub(full_arg_count);
                            let upvalues: Vec<VmValue> =
                                nc.upvalues.iter().map(|uv| uv.read(store)).collect();
                            let task = crate::task::new_lazy(
                                heap,
                                nc.proto.clone(),
                                upvalues,
                                nc.module_base,
                                staging.drain(args_start..),
                                owner_class,
                            );
                            return Ok(PreparedCall::PushValue(task));
                        }
                        if !nc.proto.is_generator && !nc.proto.is_async {
                            bundle_rest_args(&nc.proto, &mut full_arg_count, staging, heap);
                            let window: Vec<VmValue> =
                                stage_window(staging, full_arg_count).to_vec();
                            let _ = full_arg_count;
                            let mut frame = materialize_frame(store, &nc, &window)?;
                            frame.current_class = owner_class;
                            if nc.proto.name.as_deref() == Some("constructor") {
                                return Ok(PreparedCall::Constructor(frame, recv_nv));
                            }
                            return Ok(PreparedCall::Frame(frame));
                        }
                        return Err(RuntimeError::new("BoundMethod(Vm): invalid VmClosure"));
                    }
                }
            }
            HeapObj::Class(cls) => {
                let cls = cls.clone();
                let inst = varn_types::value::InstanceRef::alloc(cls.clone());
                let instance_nv = VmValue::from_heap_idx(heap.alloc(HeapObj::Instance(inst)));
                if let Some(ctor) = cls.constructor() {
                    let mut full_arg_count = arg_count;
                    if staging.is_empty() {
                        staging.push(instance_nv);
                        full_arg_count = 1;
                    } else {
                        let start = staging.len().saturating_sub(full_arg_count);
                        staging[start] = instance_nv;
                    }
                    if let Some(nc) = heap.closure_of(ctor).cloned() {
                        bundle_rest_args(&nc.proto, &mut full_arg_count, staging, heap);
                        let window: Vec<VmValue> = stage_window(staging, full_arg_count).to_vec();
                        let mut frame = materialize_frame(store, &nc, &window)?;
                        frame.current_class = Some(cls.clone());
                        return Ok(PreparedCall::Constructor(frame, instance_nv));
                    }
                    if let Some((f, _)) = heap.native_of(ctor) {
                        let take = staging.len().saturating_sub(full_arg_count);
                        let vm_args: Vec<VmValue> = staging.drain(take..).collect();
                        return Ok(PreparedCall::NativeConstructor(f, vm_args, instance_nv));
                    }
                }
                staging.clear();
                return Ok(PreparedCall::PushValue(instance_nv));
            }
            HeapObj::EnumVariant(data) => {
                let data = data.clone();
                let mut args: Vec<VmValue> = std::mem::take(staging);

                if !args.is_empty() {
                    args.remove(0);
                }

                if data.fields.is_empty() && args.is_empty() {
                    return Ok(PreparedCall::PushValue(callee_nv));
                }

                let payload = if !data.fields.is_empty() {
                    let obj = varn_types::value::ObjRef::from_pairs(
                        data.fields.iter().enumerate().map(|(idx, field_name)| {
                            let nv = args.get(idx).copied().unwrap_or(VmValue::null());
                            (field_name.clone(), nv)
                        }),
                    );
                    VmValue::from_heap_idx(heap.alloc(HeapObj::Object(obj)))
                } else if args.len() == 1 {
                    args[0]
                } else if args.len() > 1 {
                    heap.alloc_array_vm(args)
                } else {
                    VmValue::null()
                };

                let mut new_data = *data;
                new_data.payload = payload;
                return Ok(PreparedCall::PushValue(VmValue::from_heap_idx(
                    heap.alloc(HeapObj::EnumVariant(Box::new(new_data))),
                )));
            }
            _ => {}
        }
    }

    let callee_repr = heap.str_repr(callee_nv);
    let type_name = crate::exec::props::meta::type_name(callee_nv, heap);
    if callee_nv.is_null() {
        Err(RuntimeError::new(
            "Cannot invoke function because the value is null. Check if the function or host entry exists and is exported.",
        ))
    } else {
        Err(RuntimeError::new(format!(
            "value is not callable: {callee_repr} (type: {type_name})",
        )))
    }
}
