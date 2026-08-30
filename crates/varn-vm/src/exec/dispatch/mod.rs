use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_core::OpCode;
mod jit_frame;
pub(crate) mod modules;
pub(crate) mod ops_control_calls;
pub(crate) mod ops_literals_vars;
pub(crate) mod ops_math_cmp;
pub(crate) mod ops_objects_collections;
pub(crate) mod reg_ops;

#[inline(always)]
fn hi(w: u16) -> usize {
    (w >> 8) as usize
}

#[inline(always)]
fn lo(w: u16) -> usize {
    (w & 0xFF) as usize
}

impl ExecCtx {
    pub(crate) fn run(&mut self) -> VmResult<VmValue> {
        self.run_until(0)
    }

    pub(crate) fn run_until(&mut self, depth: usize) -> VmResult<VmValue> {
        self.run_until_inner(depth).map_err(|mut e| {
            if e.frames.is_empty() {
                e.frames = crate::exec::exceptions::collect_frames(&self.frames);
            }
            e
        })
    }

    pub(crate) fn run_until_inner(&mut self, depth: usize) -> VmResult<VmValue> {
        // Record this context as the CLIF static-call linking context for the
        // duration of the run: closures constructed here (and thus
        // clif-compiled) can then resolve their cross-function callees
        // against the live globals. Restored on exit so nested runs compose.
        let _link = crate::clif_link::CtxGuard::enter(self as *const ExecCtx);
        unsafe { Self::run_until_inner_raw(self as *mut ExecCtx, depth) }
    }

    #[inline(never)]
    #[allow(dangerous_implicit_autorefs)]
    unsafe fn run_until_inner_raw(ctx: *mut ExecCtx, depth: usize) -> VmResult<VmValue> {
        'frame_loop: while (*ctx).frames.len() > depth {
            let frame_idx = (*ctx).frames.len() - 1;

            let closure_ptr: *const crate::closure::VmClosure =
                (*ctx).frames[frame_idx].closure_ptr;
            let closure = unsafe { &*closure_ptr };

            let is_first_entry = (*ctx).frames[frame_idx].ip == 0;

            // On-stack replacement: `OpCode::Loop` proved this frame is
            // looping and parked the header ip. Serviced here because entering
            // compiled code means leaving the dispatch loop, and because the
            // entry itself is an ordinary `JitFn` — the whole setjmp /
            // suspend / exception / frame-pop path below is shared with a
            // normal entry rather than duplicated.
            //
            // The ip test is what binds the request to the frame that raised
            // it: an unrelated frame reached first (the request survives a
            // `continue 'frame_loop` from anywhere) must not resume at another
            // function's loop header.
            let osr_fn = match (*ctx).osr_request.take() {
                Some(osr_ip)
                    if !(*ctx).settings.no_jit && osr_ip == (*ctx).frames[frame_idx].ip =>
                {
                    closure.osr_jit_fn(osr_ip)
                }
                _ => None,
            };
            let is_osr = osr_fn.is_some();

            // Tiering point: counting entries here (rather than compiling at
            // closure construction) is what keeps a proto that is built and
            // run once out of Cranelift.
            let hot_fn = match osr_fn {
                Some(f) => Some(f),
                None if !(*ctx).settings.no_jit && is_first_entry => closure.hot_jit_fn(),
                None => None,
            };
            if let Some(jit_fn) = hot_fn {
                // Running the frame compiled, and reconciling the four ways
                // that can end, lives in `jit_frame` — it is a different job
                // from stepping opcodes and it ran once per frame entry, not
                // once per opcode.
                let outcome = jit_frame::run_compiled_frame(
                    ctx,
                    jit_fn,
                    closure_ptr,
                    closure,
                    frame_idx,
                    depth,
                    is_first_entry,
                    is_osr,
                );
                match outcome.into_result() {
                    None => continue 'frame_loop,
                    Some(result) => return result,
                }
            } else if is_first_entry {
                varn_jit::JIT_STATS
                    .interp_runs
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }

            let base = (*ctx).frames[frame_idx].base;
            let mut ip = (*ctx).frames[frame_idx].ip;
            let code_len = closure.proto.chunk.code.len();

            /// Propaga el resultado de una operación que puede fallar, dando
            /// primero a un `catch` la ocasión de manejarlo.
            ///
            /// Sin esto, un `Err` salía del bucle con `?` sin consultar nunca
            /// la tabla de excepciones —esa búsqueda vivía sólo en el brazo de
            /// `Throw`—, así que `try/catch` capturaba lo que lanzaba el
            /// usuario pero no una división por cero ni el fallo de una
            /// nativa: `JSON.parse` de una entrada inválida terminaba el
            /// proceso en vez de la petición.
            ///
            /// Sincroniza `ip` al frame antes de buscar: es una variable local
            /// del bucle y la tabla se indexa por el ip de la instrucción que
            /// falló. Sin esa escritura la búsqueda usa un ip viejo y encuentra
            /// el rango equivocado, o ninguno.
            macro_rules! tryv {
                ($e:expr) => {
                    match $e {
                        Ok(v) => v,
                        Err(err) => {
                            (*ctx).frames[frame_idx].ip = ip;
                            let mut err: crate::error::RuntimeError = err;
                            if err.frames.is_empty() {
                                err.frames = crate::exec::exceptions::collect_frames(
                                    &(*ctx).frames,
                                );
                            }
                            let tv = crate::exec::exceptions::thrown_value_for(
                                &err,
                                &mut (*ctx).heap,
                            );
                            if crate::exec::exceptions::dispatch_to_handler(ctx, tv, depth) {
                                continue 'frame_loop;
                            }
                            return Err(err);
                        }
                    }
                };
            }

            loop {
                if ip >= code_len {
                    (*ctx).frames.last_mut().unwrap().ip = ip;
                    let res = (*ctx).reg_return(base, 0);
                    if (*ctx).frames.len() == depth {
                        return Ok(res);
                    }
                    continue 'frame_loop;
                }

                let code = &closure.proto.chunk.code;
                let raw_op = code[ip];
                ip += 1;
                let first_reg = (raw_op >> 8) as usize;

                let op = match OpCode::from_u8(raw_op as u8) {
                    Some(o) => o,
                    None => {
                        return Err(crate::error::RuntimeError::new(format!(
                            "unknown opcode: {raw_op}"
                        )))
                    }
                };

                std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);

                #[cfg(feature = "profiling")]
                if let Some(counts) = (*ctx).opcode_counts.as_ref() {
                    counts[(raw_op as u8) as usize]
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }

                macro_rules! reg {
                    ($r:expr) => {
                        *({
                            let idx = base + $r;
                            #[cfg(debug_assertions)]
                            {
                                if idx >= (*ctx).stack.len() {
                                    panic!(
                                        "💥 [VM FATAL] Register Access Out Of Bounds!\n\
                                         • Function: {}\n\
                                         • Base index: {}\n\
                                         • Target Register: r{}\n\
                                         • Computed Index: {}\n\
                                         • Current Stack Size: {}\n\
                                         Please check compiler register allocation.",
                                        closure.proto.name.as_deref().unwrap_or("<anonymous>"),
                                        base,
                                        $r,
                                        idx,
                                        (*ctx).stack.len()
                                    );
                                }
                            }
                            let stack_ptr = std::ptr::addr_of_mut!((*ctx).stack);
                            #[cfg(debug_assertions)]
                            {
                                let stack_len = (*stack_ptr).len();
                                assert!(idx < stack_len, "register OOB: idx={idx} len={stack_len}");
                            }
                            (*stack_ptr).as_mut_ptr().add(idx)
                        })
                    };
                }

                match op {
                    OpCode::LoadGlobalIdx => {
                        let gidx = code[ip] as usize;
                        ip += 1;

                        reg!(first_reg) = (*ctx).globals.get_by_index_unchecked(gidx);
                        (*ctx).record_hotspot_global(gidx);
                    }
                    OpCode::LoadConst => {
                        let cidx = code[ip] as usize;
                        ip += 1;

                        debug_assert!(
                            cidx < closure.constants.len(),
                            "const index OOB: {cidx} >= {}",
                            closure.constants.len()
                        );
                        let nv = unsafe { *closure.constants.get_unchecked(cidx) };
                        reg!(first_reg) = nv;
                    }
                    OpCode::LoadNull => {
                        reg!(first_reg) = crate::value::VmValue::null();
                    }
                    OpCode::DefineGlobalIdx | OpCode::StoreGlobalIdx => {
                        let src = hi(code[ip]);
                        let gidx = code[ip + 1] as usize;
                        ip += 2;
                        let val = reg!(src);
                        (*ctx).globals.set_by_index_unchecked(gidx, val);
                    }
                    OpCode::LoadInt => {
                        let val = code[ip] as i16;
                        ip += 1;
                        reg!(first_reg) = crate::value::VmValue::from_int(val as i64);
                    }
                    OpCode::Move => {
                        let w1 = code[ip];
                        ip += 1;
                        let dest = base + first_reg;
                        let src = base + hi(w1);
                        let max_idx = dest.max(src);
                        if max_idx >= (*ctx).stack.len() {
                            (*ctx)
                                .stack
                                .resize(max_idx + 1, crate::value::VmValue::null());
                        }
                        (*ctx).stack[dest] = (*ctx).stack[src];
                    }
                    OpCode::LoadGlobal
                    | OpCode::StoreGlobal
                    | OpCode::DefineGlobal
                    | OpCode::LoadUpvalue
                    | OpCode::StoreUpvalue
                    | OpCode::CloseUpvalue
                    | OpCode::LoadTrue
                    | OpCode::LoadFalse
                    | OpCode::LoadIntZero
                    | OpCode::LoadIntOne
                    | OpCode::LoadIntMinusOne => {
                        let handled = tryv!((*ctx).exec_literals_vars_op(
                            op, code, &mut ip, base, frame_idx, closure, first_reg,
                        ));
                        debug_assert!(handled, "exec_literals_vars_op must handle grouped opcodes");
                    }

                    OpCode::Add
                    | OpCode::Sub
                    | OpCode::Mul
                    | OpCode::Div
                    | OpCode::Mod
                    | OpCode::Pow
                    | OpCode::BitAnd
                    | OpCode::BitOr
                    | OpCode::BitXor
                    | OpCode::Shl
                    | OpCode::Shr
                    | OpCode::Ushr
                    | OpCode::Negate
                    | OpCode::Not
                    | OpCode::AddImm
                    | OpCode::SubImm
                    | OpCode::AddInt
                    | OpCode::SubInt
                    | OpCode::MulInt
                    | OpCode::DivInt
                    | OpCode::ModInt
                    | OpCode::PowInt
                    | OpCode::LtInt
                    | OpCode::GtInt
                    | OpCode::LteInt
                    | OpCode::GteInt
                    | OpCode::EqInt
                    | OpCode::NeqInt
                    | OpCode::AddFloat
                    | OpCode::SubFloat
                    | OpCode::MulFloat
                    | OpCode::DivFloat
                    | OpCode::ModFloat
                    | OpCode::PowFloat
                    | OpCode::LtFloat
                    | OpCode::GtFloat
                    | OpCode::LteFloat
                    | OpCode::GteFloat
                    | OpCode::EqFloat
                    | OpCode::NeqFloat
                    | OpCode::Eq
                    | OpCode::Neq
                    | OpCode::Lt
                    | OpCode::Lte
                    | OpCode::Gt
                    | OpCode::Gte
                    | OpCode::ToString
                    | OpCode::StrConcat
                    | OpCode::BuildStr
                    | OpCode::StrLength
                    | OpCode::StrSlice => {
                        let handled = tryv!((*ctx).exec_math_cmp_op(
                            op, code, &mut ip, base, frame_idx, closure, first_reg,
                        ));
                        debug_assert!(handled, "exec_math_cmp_op must handle grouped opcodes");
                    }

                    OpCode::Jump
                    | OpCode::Loop
                    | OpCode::JumpIfFalse
                    | OpCode::JumpIfTrue
                    | OpCode::Return
                    | OpCode::Call
                    | OpCode::CallSelf
                    | OpCode::CallMethod
                    | OpCode::InvokeVirtual
                    | OpCode::CallSpread => {
                        if let Some(flow) = tryv!((*ctx).exec_control_calls_op(
                            op, code, &mut ip, base, frame_idx, closure, first_reg, depth,
                        )) {
                            match flow {
                                crate::exec::dispatch::ops_control_calls::ControlCallFlow::ContinueInstruction => {}
                                crate::exec::dispatch::ops_control_calls::ControlCallFlow::ContinueFrame => {
                                    continue 'frame_loop;
                                }
                                crate::exec::dispatch::ops_control_calls::ControlCallFlow::Return(v) => {
                                    return Ok(v);
                                }
                            }
                        }
                    }

                    OpCode::MakeClosure
                    | OpCode::GetProperty
                    | OpCode::GetPropertyMaybe
                    | OpCode::SetProperty
                    | OpCode::GetFixedField
                    | OpCode::SetFixedField
                    | OpCode::GetSuper
                    | OpCode::GetSymbol
                    | OpCode::AssertNotNull
                    | OpCode::DeclareField
                    | OpCode::GetIndex
                    | OpCode::SetIndex
                    | OpCode::ArrayGetIndex
                    | OpCode::ArraySetIndex
                    | OpCode::BuildArray
                    | OpCode::BuildTuple
                    | OpCode::BuildObject
                    | OpCode::BuildObjectWithShape
                    | OpCode::BuildRecord
                    | OpCode::ObjectRest
                    | OpCode::ObjectKeys
                    | OpCode::ObjectMerge
                    | OpCode::WrapSpread
                    | OpCode::ArrayLength
                    | OpCode::ArrayPush
                    | OpCode::ArrayPop
                    | OpCode::ArrayExtend
                    | OpCode::In
                    | OpCode::Instanceof
                    | OpCode::Typeof
                    | OpCode::IsNull
                    | OpCode::IsArray => {
                        if let Some(flow) = tryv!((*ctx).exec_objects_collections_op(
                            op, code, &mut ip, base, frame_idx, closure, first_reg,
                        )) {
                            match flow {
                                crate::exec::dispatch::ops_objects_collections::ObjectFlow::ContinueInstruction => {}
                                crate::exec::dispatch::ops_objects_collections::ObjectFlow::ContinueFrame => {
                                    continue 'frame_loop;
                                }
                            }
                        }
                    }

                    OpCode::MakeClass
                    | OpCode::Inherit
                    | OpCode::Method
                    | OpCode::DefineStatic
                    | OpCode::DefineGetter
                    | OpCode::DefineSetter
                    | OpCode::DefineStaticGetter
                    | OpCode::DefineStaticSetter
                    | OpCode::BindMethod => {
                        (*ctx).frames[frame_idx].ip = ip;
                        tryv!((*ctx).exec_class_op(
                            op, code, &mut ip, base, frame_idx, closure, first_reg,
                        ));
                        let frame_idx2 = (*ctx).frames.len() - 1;
                        ip = (*ctx).frames[frame_idx2].ip;
                    }
                    OpCode::MakeEnumVariant => {
                        (*ctx).frames[frame_idx].ip = ip;
                        tryv!((*ctx)
                            .exec_make_enum_variant_reg(code, &mut ip, base, frame_idx, closure));
                        let frame_idx2 = (*ctx).frames.len() - 1;
                        ip = (*ctx).frames[frame_idx2].ip;
                    }
                    OpCode::GetEnumTag => {
                        let src = hi(code[ip]);
                        ip += 1;
                        let v = reg![src];
                        reg![first_reg] = tryv!((*ctx).exec_get_enum_tag(v));
                    }

                    OpCode::Try => {
                        let w1 = code[ip];
                        ip += 1;
                        let err_reg = hi(w1) as u8;
                        let offset_hi = code[ip] as u32;
                        let offset_lo = code[ip + 1] as u32;
                        let catch_offset = ((offset_hi << 16) | offset_lo) as usize;
                        ip += 2;
                        let catch_ip = ip + catch_offset;
                        crate::exec::exceptions::push_try(
                            &mut (*ctx).try_handlers,
                            catch_ip,
                            (*ctx).frames.len(),
                            err_reg,
                        );
                    }
                    OpCode::PopTry => {
                        crate::exec::exceptions::pop_try(&mut (*ctx).try_handlers);
                    }
                    OpCode::Throw => {
                        let w1 = code[ip];
                        let src = hi(w1);
                        let val = reg![src];
                        (*ctx).frames[frame_idx].ip = ip;
                        let err = crate::exec::exceptions::build_thrown_error(
                            val,
                            &(*ctx).heap,
                            &(*ctx).frames,
                        );
                        let thrown_val = err.thrown.unwrap_or(VmValue::null());
                        if crate::exec::exceptions::dispatch_to_handler(ctx, thrown_val, depth) {
                            continue 'frame_loop;
                        }
                        return Err(err);
                    }

                    OpCode::Yield => {
                        let w1 = code[ip];
                        ip += 1;
                        let dest = hi(w1) as u8;
                        let src = lo(w1);
                        let val = reg![src];
                        (*ctx).frames[frame_idx].ip = ip;
                        (*ctx).vm_suspend = Some(crate::exec::VmSuspend::Yield {
                            value: val,
                            dest_reg: dest,
                        });
                        return Ok(VmValue::null());
                    }
                    OpCode::Await => {
                        let src = hi(code[ip]);
                        ip += 1;
                        let fut = reg![src];
                        (*ctx).frames[frame_idx].ip = ip;
                        (*ctx).vm_suspend = Some(crate::exec::VmSuspend::Await {
                            value: (*ctx).heap.extract(fut),
                            dest_reg: first_reg as u16,
                        });
                        return Ok(VmValue::null());
                    }
                    OpCode::Spawn => {
                        let w1 = code[ip];
                        ip += 1;
                        let (dest, src) = (first_reg, hi(w1));

                        let src_slot = base + src;
                        if src_slot >= (*ctx).stack.len() {
                            (*ctx)
                                .stack
                                .resize(src_slot + 1, crate::value::VmValue::null());
                        }
                        let task_val = reg![src];
                        let dest_slot = base + dest;
                        if dest_slot >= (*ctx).stack.len() {
                            (*ctx)
                                .stack
                                .resize(dest_slot + 1, crate::value::VmValue::null());
                        }
                        reg![dest] = tryv!((*ctx).exec_spawn(task_val));
                    }

                    OpCode::LoadModule | OpCode::LoadModuleSlot | OpCode::StoreModuleSlot => {
                        (*ctx).frames[frame_idx].ip = ip;
                        tryv!((*ctx).exec_module_op_reg(
                            op, code, &mut ip, frame_idx, closure, first_reg,
                        ));
                        ip = (*ctx).frames[frame_idx].ip;
                    }

                    OpCode::InvokeRuntimeStatic => {
                        (*ctx).frames[frame_idx].ip = ip;
                        tryv!((*ctx).exec_invoke_runtime_static_reg(
                            code, &mut ip, base, frame_idx, closure,
                        ));
                        let frame_idx2 = (*ctx).frames.len() - 1;
                        ip = (*ctx).frames[frame_idx2].ip;
                    }
                    OpCode::Intrinsic => {
                        let w1 = code[ip];
                        ip += 1;
                        let wire_byte = (w1 >> 8) as u8;

                        let arg_count = (w1 & 0xFF) as usize;
                        let args_start = base + first_reg;
                        let result = tryv!(crate::exec::intrinsics::dispatch(
                            wire_byte,
                            &(*ctx).stack[args_start..args_start + arg_count],
                            &mut (*ctx).heap,
                        ));
                        (*ctx).stack[base + first_reg] = result;
                    }

                    OpCode::IntrinsicDirect => {
                        let w1 = code[ip];
                        ip += 1;
                        let src = (w1 >> 8) as usize;
                        let wire_byte = (w1 & 0xFF) as u8;
                        let x = (*ctx).stack[base + src];
                        let result = tryv!(crate::exec::intrinsics::dispatch_unary(wire_byte, x));
                        (*ctx).stack[base + first_reg] = result;
                    }

                    OpCode::CallNativeOp => {
                        let cidx = code[ip] as usize;
                        let total = code[ip + 1] as usize; // receiver + args
                        ip += 2;
                        // op-id is stored as a full i64 constant (NOT the
                        // NaN-boxed `closure.constants` cache, which truncates).
                        let op_id = match closure.proto.chunk.constants.get(cidx) {
                            Some(varn_types::chunk::PoolEntry::Literal(
                                varn_types::chunk::Literal::Int(i),
                            )) => *i as u64,
                            _ => {
                                return Err(crate::error::RuntimeError::new(format!(
                                    "CallNativeOp: const {cidx} is not an op-id"
                                )))
                            }
                        };
                        let f = tryv!(varn_builtins::native_op_fn(op_id).ok_or_else(|| {
                            crate::error::RuntimeError::new(format!(
                                "CallNativeOp: unknown op-id {op_id}"
                            ))
                        }));
                        let receiver = (*ctx).stack[base + first_reg];
                        // Reuse the exact native-call path the inline cache uses,
                        // so error and profiling semantics are identical.
                        let result = tryv!((*ctx).call_native_with_receiver(
                            f,
                            receiver,
                            base,
                            first_reg + 1,
                            total - 1,
                        ));
                        let target_slot = base + first_reg;
                        if (*ctx).stack.len() <= target_slot {
                            (*ctx).stack.resize(target_slot + 1, VmValue::null());
                        }
                        (*ctx).stack[target_slot] = result;
                    }

                    OpCode::LoadStaticFn => {
                        let proto_idx = code[ip] as usize;
                        ip += 1;
                        let dest = first_reg;
                        let proto = match closure.proto.chunk.constants.get(proto_idx) {
                            Some(varn_types::PoolEntry::Function(p)) => p,
                            _ => {
                                return Err(crate::error::RuntimeError::new(format!(
                                    "LoadStaticFn: const {proto_idx} is not a function"
                                )))
                            }
                        };

                        let proto_ptr = std::rc::Rc::as_ptr(proto) as usize;
                        let val = if let Some(&(_, cached_val)) =
                            (*ctx).static_closures.get(&proto_ptr)
                        {
                            cached_val
                        } else {
                            let constants = std::rc::Rc::new(
                                crate::exec::calls::resolve_constants(proto, &mut (*ctx).heap),
                            );
                            let vm_closure =
                                std::rc::Rc::new(crate::closure::VmClosure::with_upvalues(
                                    proto.clone(),
                                    vec![],
                                    constants,
                                    (*ctx).settings,
                                ));
                            let val = (*ctx).heap.alloc_vm_closure(vm_closure);
                            (*ctx)
                                .static_closures
                                .insert(proto_ptr, (proto.clone(), val));
                            val
                        };
                        (*ctx).stack[base + dest] = val;
                    }

                    OpCode::Nop => {}
                }

                if (*ctx).vm_suspend.is_some() {
                    (*ctx).frames[frame_idx].ip = ip;
                    return Ok(VmValue::null());
                }
            }
        }
        Ok(VmValue::null())
    }

    pub(super) fn reg_return(&mut self, base: usize, src: usize) -> VmValue {
        let val = self.stack[base + src];
        let returning_frame_idx = self.frames.len().saturating_sub(1);
        let frame = self.frames.pop().unwrap();
        self.close_upvalues_above(frame.base);

        let is_module_frame = frame.closure().proto.name.as_deref() == Some("<module>")
            && !frame.closure().proto.chunk.source_file.is_empty();

        if let Some(caller) = self.frames.last() {
            let caller_req = caller.base + caller.closure().proto.register_count as usize;
            if self.stack.len() < caller_req {
                self.stack.resize(caller_req, VmValue::null());
            } else {
                self.stack.truncate(caller_req);
            }
        }

        let final_val =
            crate::exec::frame_ctrl::resolve_constructor_return(self, returning_frame_idx, val);

        if is_module_frame {
            let source_file = frame.closure().proto.chunk.source_file.to_string();
            let module_exports = self.module_exports.remove(&returning_frame_idx);
            let cached = module_exports.unwrap_or(final_val);
            let module_id = varn_core::ModuleId::from_canonical_str(&source_file);
            self.modules.insert(module_id, cached);
        }

        if let Some(return_reg) = frame.return_reg {
            let caller_base = self.frames.last().map(|f| f.base).unwrap_or(0);
            self.stack[caller_base + return_reg as usize] = final_val;
        }
        final_val
    }


    pub(crate) fn exec_typeof(&self, v: VmValue) -> &'static str {
        use varn_core::TypeTag;
        if v.is_null() {
            return TypeTag::Null.name();
        }
        if v.is_int() {
            return TypeTag::Int.name();
        }
        if v.is_f64() {
            return TypeTag::Float.name();
        }
        if v.is_bool() {
            return TypeTag::Bool.name();
        }
        if v.is_sso() {
            return TypeTag::Str.name();
        }
        if !v.is_heap() {
            return "unknown";
        }
        match self.heap.get(v.as_heap_idx()) {
            Some(obj) => obj.tag().name(),
            None => "unknown",
        }
    }
}
