use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_core::OpCode;
pub mod modules;
pub mod ops_control_calls;
pub mod ops_literals_vars;
pub mod ops_math_cmp;
pub mod ops_objects_collections;
pub mod reg_ops;

#[derive(Debug, PartialEq, Eq)]
pub enum ControlSignal {
    ContinueInstruction,
    ContinueFrame,
    Return(VmValue),
    None,
}

#[inline(always)]
fn hi(w: u16) -> usize {
    (w >> 8) as usize
}

#[inline(always)]
fn lo(w: u16) -> usize {
    (w & 0xFF) as usize
}

impl ExecCtx {
    pub fn run(&mut self) -> VmResult<VmValue> {
        self.run_until(0)
    }

    pub fn run_until(&mut self, depth: usize) -> VmResult<VmValue> {
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

    /// Run one clif frame under its OWN jump buffer.
    ///
    /// Every clif frame installs one, not just the outermost. A clif frame is
    /// a native stack frame, so a `throw` raised several clif frames deep used
    /// to `longjmp` straight past the ones in between — destroying their
    /// native state while their `CallFrame`s stayed live with `ip == 0`, which
    /// the frame loop then re-entered from the top, forever. (`assert(
    /// safeDivide(10, 0) === -1)` in tests/11-errors.vn hung allocating.)
    ///
    /// With a buffer per frame the unwind is one hop: the innermost frame
    /// catches, its own frame loop handles the throw if the handler belongs at
    /// or above it, and otherwise returns the error to `clif_call_fallback`,
    /// which re-raises against the buffer this function has by then RESTORED
    /// to its parent's. Each clif frame therefore unwinds its own native stack
    /// in turn. A null saved buffer means we are outermost: the error leaves
    /// as `Err`, exactly as before.
    ///
    /// Suspension is the one thing that still needs the OUTERMOST buffer — an
    /// intermediate clif frame cannot park in the middle of a native function
    /// — so `jit_suspend_buf` is pinned by the outermost frame and is what
    /// `jit_await`/`jit_yield` jump to. That preserves today's behaviour
    /// exactly; it does not make suspension nest.
    #[inline(never)]
    unsafe fn execute_jit_frame(
        ctx: *mut ExecCtx,
        jit_fn: varn_jit::JitFn,
        closure_ptr: *const crate::frame::VmClosure,
        base: usize,
    ) -> Result<VmValue, i32> {
        let saved = (*ctx).jit_jmp_buf;
        let is_outer = saved.is_null();
        let mut jmp_buf = super::ctx::JmpBuf::default();
        let jmp_res = std::hint::black_box(super::ctx::my_setjmp(&mut jmp_buf));

        if jmp_res == 0 {
            (*ctx).jit_jmp_buf = &mut jmp_buf as *mut super::ctx::JmpBuf;
            if is_outer {
                (*ctx).jit_suspend_buf = (*ctx).jit_jmp_buf;
            }
            (*ctx).jit_frame_prepushed = 1;
            // Explicit borrow of the `Rc` field: an implicit autoref through a
            // raw pointer is what `dangerous_implicit_autorefs` warns about.
            let required = base + (&(*closure_ptr).proto).register_count as usize;
            if (*ctx).stack.len() < required {
                (*ctx).stack.resize(required, crate::value::VmValue::null());
            }
            let val = (jit_fn)(
                (*ctx).stack.as_mut_ptr() as *mut std::ffi::c_void,
                closure_ptr as *const std::ffi::c_void,
                base,
                ctx as *mut std::ffi::c_void,
            );
            std::hint::black_box(ctx);
            (*ctx).jit_jmp_buf = saved;
            if is_outer {
                (*ctx).jit_suspend_buf = std::ptr::null_mut();
            }
            Ok(val)
        } else {
            (*ctx).jit_jmp_buf = saved;
            if is_outer {
                (*ctx).jit_suspend_buf = std::ptr::null_mut();
            }
            Err(jmp_res)
        }
    }

    #[inline(never)]
    #[allow(dangerous_implicit_autorefs)]
    unsafe fn run_until_inner_raw(ctx: *mut ExecCtx, depth: usize) -> VmResult<VmValue> {
        'frame_loop: while (*ctx).frames.len() > depth {
            let frame_idx = (*ctx).frames.len() - 1;

            let closure_ptr: *const crate::frame::VmClosure = (*ctx).frames[frame_idx].closure_ptr;
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
                if is_first_entry {
                    varn_jit::JIT_STATS
                        .jit_runs
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                if is_osr {
                    // Not counted in `jit_runs`: this frame already counted as
                    // an interpreted entry when it started, and it is the same
                    // frame. `osr_entries` is what says the rescue happened.
                    varn_jit::JIT_STATS
                        .osr_entries
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                if (*ctx).settings.trace {
                    let what = if is_osr { "JIT OSR" } else { "JIT ENTRY" };
                    let at = (*ctx).frames[frame_idx].ip;
                    (*ctx).trace_event(what, frame_idx, closure, at, None);
                }

                let base = (*ctx).frames[frame_idx].base;
                let res = Self::execute_jit_frame(ctx, jit_fn, closure_ptr, base);

                let res = match res {
                    Ok(val) => val,
                    Err(code) => {
                        if code == 1 {
                            let handler = (*ctx).jit_panic_exception_handler.take();
                            let error = (*ctx)
                                .jit_panic_exception_error
                                .take()
                                .unwrap_or(VmValue::null());
                            let err_obj = (*ctx).jit_panic_exception_err_obj.take();

                            if let Some(handler) = handler {
                                while (*ctx).frames.len() > handler.frame_depth {
                                    (*ctx).record_frame_pop();
                                    let f = (*ctx).frames.pop().unwrap();
                                    (*ctx).close_upvalues_above(f.base);
                                }

                                let f2 = (*ctx).frames.len() - 1;
                                let b2 = (*ctx).frames[f2].base;
                                let required_depth =
                                    b2 + (*ctx).frames[f2].closure().proto.register_count as usize;
                                (*ctx).stack.truncate(required_depth);
                                let thrown_val = error;

                                let slot = b2 + handler.err_reg as usize;
                                if slot < (*ctx).stack.len() {
                                    (*ctx).stack[slot] = thrown_val;
                                } else {
                                    (*ctx).stack.resize(slot + 1, VmValue::null());
                                    (*ctx).stack[slot] = thrown_val;
                                }
                                let new_frame_idx = (*ctx).frames.len() - 1;
                                (*ctx).frames[new_frame_idx].ip = handler.catch_ip;
                                continue 'frame_loop;
                            } else {
                                return Err(err_obj.unwrap());
                            }
                        } else if code == 2 {
                            // Absent when the suspending helper parked a frame
                            // other than the top one itself (see
                            // `jit_suspend_at`): a module that suspends on a
                            // top-level await leaves its frame above ours.
                            if let Some(resume_ip) = (*ctx).jit_panic_suspend_resume_ip.take() {
                                let frame_idx2 = (*ctx).frames.len() - 1;
                                (*ctx).frames[frame_idx2].ip = resume_ip;
                            }
                            return Ok(VmValue::null());
                        } else {
                            panic!("Unknown longjmp code: {}", code);
                        }
                    }
                };
                if (*ctx).settings.trace {
                    (*ctx).trace_event("JIT EXIT", frame_idx, closure, 0, None);
                }

                let frame = (*ctx).frames.pop().unwrap();
                (*ctx).record_frame_pop();
                while (*ctx)
                    .try_handlers
                    .last()
                    .map(|h| h.frame_depth > (*ctx).frames.len())
                    .unwrap_or(false)
                {
                    (*ctx).try_handlers.pop();
                }
                (*ctx).close_upvalues_above(frame.base);
                (*ctx).stack.truncate(frame.base);

                let ctor_pos = if !(*ctx).pending_constructors.is_empty() {
                    (*ctx)
                        .pending_constructors
                        .iter()
                        .rposition(|(idx, _)| *idx == frame_idx)
                } else {
                    None
                };

                let final_val = if let Some(pos) = ctor_pos {
                    let (_, instance_nv) = (*ctx).pending_constructors.remove(pos);
                    if res.is_null() {
                        instance_nv
                    } else {
                        res
                    }
                } else {
                    res
                };

                if let Some(return_reg) = frame.return_reg {
                    let caller_base = (*ctx).frames.last().map(|f| f.base).unwrap_or(0);
                    (*ctx).stack[caller_base + return_reg as usize] = final_val;
                }

                if (*ctx).frames.len() == depth {
                    return Ok(final_val);
                }
                continue 'frame_loop;
            } else {
                if is_first_entry {
                    varn_jit::JIT_STATS
                        .interp_runs
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }

            let base = (*ctx).frames[frame_idx].base;
            let mut ip = (*ctx).frames[frame_idx].ip;
            let code_len = closure.proto.chunk.code.len();

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
                        let src = base + hi(w1) as usize;
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
                        let handled = (*ctx).exec_literals_vars_op(
                            op, code, &mut ip, base, frame_idx, &closure, first_reg,
                        )?;
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
                        let handled = (*ctx).exec_math_cmp_op(
                            op, code, &mut ip, base, frame_idx, &closure, first_reg,
                        )?;
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
                        if let Some(flow) = (*ctx).exec_control_calls_op(
                            op, code, &mut ip, base, frame_idx, &closure, first_reg, depth,
                        )? {
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
                    | OpCode::BuildObject
                    | OpCode::BuildObjectWithShape
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
                        if let Some(flow) = (*ctx).exec_objects_collections_op(
                            op, code, &mut ip, base, frame_idx, &closure, first_reg,
                        )? {
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
                        (*ctx).exec_class_op(
                            op, code, &mut ip, base, frame_idx, &closure, first_reg,
                        )?;
                        let frame_idx2 = (*ctx).frames.len() - 1;
                        ip = (*ctx).frames[frame_idx2].ip;
                    }
                    OpCode::MakeEnumVariant => {
                        (*ctx).frames[frame_idx].ip = ip;
                        (*ctx)
                            .exec_make_enum_variant_reg(code, &mut ip, base, frame_idx, &closure)?;
                        let frame_idx2 = (*ctx).frames.len() - 1;
                        ip = (*ctx).frames[frame_idx2].ip;
                    }
                    OpCode::GetEnumTag => {
                        let src = hi(code[ip]);
                        ip += 1;
                        let v = reg![src];
                        reg![first_reg] = (*ctx).exec_get_enum_tag(v)?;
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
                        let err = crate::exec::exceptions::build_thrown_error(
                            val,
                            &(*ctx).heap,
                            &(*ctx).frames,
                        );
                        if let Some(handler) = (*ctx).try_handlers.pop() {
                            while (*ctx).frames.len() > handler.frame_depth {
                                (*ctx).record_frame_pop();
                                let f = (*ctx).frames.pop().unwrap();
                                (*ctx).close_upvalues_above(f.base);
                            }

                            let f2 = (*ctx).frames.len() - 1;
                            let b2 = (*ctx).frames[f2].base;
                            let required_depth =
                                b2 + (*ctx).frames[f2].closure().proto.register_count as usize;
                            (*ctx).stack.truncate(required_depth);
                            let thrown_val = err.thrown.unwrap_or(VmValue::null());

                            let slot = b2 + handler.err_reg as usize;
                            if slot < (*ctx).stack.len() {
                                (*ctx).stack[slot] = thrown_val;
                            } else {
                                (*ctx).stack.resize(slot + 1, VmValue::null());
                                (*ctx).stack[slot] = thrown_val;
                            }
                            let new_frame_idx = (*ctx).frames.len() - 1;
                            (*ctx).frames[new_frame_idx].ip = handler.catch_ip;
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

                        let src_slot = base + src as usize;
                        if src_slot >= (*ctx).stack.len() {
                            (*ctx)
                                .stack
                                .resize(src_slot + 1, crate::value::VmValue::null());
                        }
                        let task_val = reg![src];
                        let dest_slot = base + dest as usize;
                        if dest_slot >= (*ctx).stack.len() {
                            (*ctx)
                                .stack
                                .resize(dest_slot + 1, crate::value::VmValue::null());
                        }
                        reg![dest] = (*ctx).exec_spawn(task_val)?;
                    }

                    OpCode::LoadModule | OpCode::LoadModuleSlot | OpCode::StoreModuleSlot => {
                        (*ctx).frames[frame_idx].ip = ip;
                        (*ctx).exec_module_op_reg(
                            op, code, &mut ip, frame_idx, &closure, first_reg,
                        )?;
                        ip = (*ctx).frames[frame_idx].ip;
                    }

                    OpCode::InvokeRuntimeStatic => {
                        (*ctx).frames[frame_idx].ip = ip;
                        (*ctx).exec_invoke_runtime_static_reg(
                            code, &mut ip, base, frame_idx, &closure,
                        )?;
                        let frame_idx2 = (*ctx).frames.len() - 1;
                        ip = (*ctx).frames[frame_idx2].ip;
                    }
                    OpCode::Intrinsic => {
                        let w1 = code[ip];
                        ip += 1;
                        let wire_byte = (w1 >> 8) as u8;

                        let arg_count = (w1 & 0xFF) as usize;
                        let args_start = base + first_reg;
                        let result = crate::exec::intrinsics::dispatch(
                            wire_byte,
                            &(*ctx).stack[args_start..args_start + arg_count],
                            &mut (*ctx).heap,
                        )?;
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
                        let f = varn_builtins::native_op_fn(op_id).ok_or_else(|| {
                            crate::error::RuntimeError::new(format!(
                                "CallNativeOp: unknown op-id {op_id}"
                            ))
                        })?;
                        let receiver = (*ctx).stack[base + first_reg];
                        // Reuse the exact native-call path the inline cache uses,
                        // so error and profiling semantics are identical.
                        let result = (*ctx).call_native_with_receiver(
                            f,
                            receiver,
                            base,
                            first_reg + 1,
                            total - 1,
                        )?;
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
                                std::rc::Rc::new(crate::frame::VmClosure::with_upvalues(
                                    proto.clone(),
                                    vec![],
                                    constants,
                                    (*ctx).settings,
                                ));
                            let val = (*ctx).heap.alloc_vm_closure(vm_closure);
                            (*ctx).static_closures.insert(proto_ptr, (proto.clone(), val));
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
        self.record_frame_pop();
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

        let ctor_pos = if !self.pending_constructors.is_empty() {
            self.pending_constructors
                .iter()
                .rposition(|(idx, _)| *idx == returning_frame_idx)
        } else {
            None
        };

        let final_val = if let Some(pos) = ctor_pos {
            let (_, instance_nv) = self.pending_constructors.remove(pos);
            if val.is_null() {
                instance_nv
            } else {
                val
            }
        } else {
            val
        };

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

    pub(super) fn exec_arith(&mut self, op: OpCode, a: VmValue, b: VmValue) -> VmResult<VmValue> {
        use crate::exec::arith;
        match op {
            OpCode::Add => arith::add(a, b, &mut self.heap),
            OpCode::Sub => arith::sub(a, b, &mut self.heap),
            OpCode::Mul => arith::mul(a, b, &mut self.heap),
            OpCode::Div => arith::div(a, b, &mut self.heap),
            OpCode::Mod => arith::modulo(a, b, &mut self.heap),
            OpCode::Pow => arith::pow(a, b, &mut self.heap),
            OpCode::BitAnd => Ok(arith::bit_and(a, b, &mut self.heap)),
            OpCode::BitOr => Ok(arith::bit_or(a, b, &mut self.heap)),
            OpCode::BitXor => Ok(arith::bit_xor(a, b, &mut self.heap)),
            OpCode::Shl => Ok(arith::shl(a, b, &mut self.heap)),
            OpCode::Shr => Ok(arith::shr(a, b, &mut self.heap)),
            OpCode::Ushr => Ok(arith::ushr(a, b, &mut self.heap)),
            _ => unreachable!(),
        }
    }

    pub(super) fn exec_cmp(&mut self, op: OpCode, a: VmValue, b: VmValue) -> VmValue {
        use crate::exec::compare;
        VmValue::from_bool(match op {
            OpCode::Eq => compare::eq(a, b, &self.heap),
            OpCode::Neq => compare::neq(a, b, &self.heap),
            OpCode::Lt => compare::lt_heap(a, b, &self.heap),
            OpCode::Lte => compare::lte_heap(a, b, &self.heap),
            OpCode::Gt => compare::gt_heap(a, b, &self.heap),
            OpCode::Gte => compare::gte_heap(a, b, &self.heap),
            _ => unreachable!(),
        })
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
        if !v.is_heap() {
            return "unknown";
        }
        match self.heap.get(v.as_heap_idx()) {
            Some(obj) => obj.tag().name(),
            None => "unknown",
        }
    }
}
