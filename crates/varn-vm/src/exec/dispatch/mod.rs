use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use opgroups::*;
use varn_core::OpCode;
mod frame_entry;
mod jit_frame;
pub(crate) mod modules;
mod opgroups;
mod ops_build;
pub(crate) mod ops_control_calls;
pub(crate) mod ops_literals_vars;
pub(crate) mod ops_math_cmp;
mod ops_misc;
mod ops_native;
pub(crate) mod ops_objects_collections;
pub(crate) mod reg_ops;
mod returns;

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
        let _link = crate::clif_link::CtxGuard::enter(self as *const ExecCtx);
        unsafe { Self::run_until_inner_raw(self as *mut ExecCtx, depth) }
    }

    #[inline(never)]
    #[allow(dangerous_implicit_autorefs)]
    unsafe fn run_until_inner_raw(ctx: *mut ExecCtx, depth: usize) -> VmResult<VmValue> {
        'frame_loop: while unsafe { (*ctx).frames.len() } > depth {
            let frame_idx = unsafe { (*ctx).frames.len() } - 1;

            let closure_ptr: *const crate::closure::VmClosure =
                unsafe { (*ctx).frames[frame_idx].closure_ptr };
            let closure = unsafe { &*closure_ptr };

            match unsafe { frame_entry::enter(ctx, closure_ptr, closure, frame_idx, depth) } {
                frame_entry::Entry::Interpret => {}
                frame_entry::Entry::NextFrame => continue 'frame_loop,
                frame_entry::Entry::Done(result) => return result,
            }

            let base = unsafe { (*ctx).frames[frame_idx].base };
            let mut ip = unsafe { (*ctx).frames[frame_idx].ip };
            let code_len = closure.proto.chunk.code.len();

            macro_rules! tryv {
                ($e:expr) => {
                    match $e {
                        Ok(v) => v,
                        Err(err) => {
                            unsafe { (*ctx).frames[frame_idx].ip = ip };
                            match unsafe {
                                crate::exec::exceptions::handle_runtime_error(ctx, err, depth)
                            } {
                                Ok(()) => continue 'frame_loop,
                                Err(err) => return Err(err),
                            }
                        }
                    }
                };
            }

            loop {
                if ip >= code_len {
                    unsafe { (*ctx).frames.last_mut().unwrap().ip = ip };
                    let res = tryv!(unsafe { (*ctx).reg_return(base, 0) });
                    if unsafe { (*ctx).frames.len() } == depth {
                        return Ok(res);
                    }
                    continue 'frame_loop;
                }

                if crate::debug::check_break(ctx, closure_ptr, frame_idx, ip) {
                    return Ok(VmValue::null());
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
                if let Some(counts) = unsafe { (*ctx).opcode_counts.as_ref() } {
                    counts[(raw_op as u8) as usize]
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }

                macro_rules! reg_box {
                    ($r:expr) => {
                        unsafe { (*ctx).stack.box_reg(base, $r) }
                    };
                }

                match op {
                    OpCode::LoadGlobalIdx => {
                        let gidx = closure.module_base as usize + code[ip] as usize;
                        ip += 1;

                        let nv = unsafe { (*ctx).globals_ref().get_by_index_unchecked(gidx) };
                        tryv!(unsafe { (*ctx).stack.unbox_into_reg(base, first_reg, nv) });
                        unsafe { (*ctx).record_hotspot_global(gidx) };
                    }
                    OpCode::LoadNativeGlobalIdx => {
                        let gidx = code[ip] as usize;
                        ip += 1;

                        let nv = unsafe { (*ctx).globals_ref().get_by_index_unchecked(gidx) };
                        tryv!(unsafe { (*ctx).stack.unbox_into_reg(base, first_reg, nv) });
                        unsafe { (*ctx).record_hotspot_global(gidx) };
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
                        tryv!(unsafe { (*ctx).stack.unbox_into_reg(base, first_reg, nv) });
                    }
                    OpCode::LoadNull => {
                        tryv!(unsafe {
                            (*ctx).stack.unbox_into_reg(
                                base,
                                first_reg,
                                crate::value::VmValue::null(),
                            )
                        });
                    }
                    OpCode::DefineGlobalIdx | OpCode::StoreGlobalIdx => {
                        let src = hi(code[ip]);
                        let gidx = closure.module_base as usize + code[ip + 1] as usize;
                        ip += 2;
                        let val = reg_box!(src);
                        unsafe { (*ctx).globals_mut().set_by_index_unchecked(gidx, val) };
                    }
                    OpCode::LoadInt => {
                        let val = code[ip] as i16;
                        ip += 1;
                        tryv!(unsafe {
                            (*ctx).stack.unbox_into_reg(
                                base,
                                first_reg,
                                crate::value::VmValue::from_int(val as i64),
                            )
                        });
                    }
                    OpCode::Move => {
                        let w1 = code[ip];
                        ip += 1;
                        tryv!(unsafe { (*ctx).stack.mov(base, first_reg, hi(w1)) });
                    }
                    OpCode::Convert => {
                        let w1 = code[ip];
                        ip += 1;
                        let v = unsafe { (*ctx).stack.box_reg(base, hi(w1)) };
                        let r = match varn_core::ValueConv::from_u8(lo(w1) as u8) {
                            Some(conv) => {
                                crate::exec::convert::convert(conv, v, unsafe { &mut (*ctx).heap })
                            }
                            None => Err(crate::error::RuntimeError::new("convert: bad operand")),
                        };
                        let r = tryv!(r);
                        tryv!(unsafe { (*ctx).stack.unbox_into_reg(base, first_reg, r) });
                    }
                    literals_vars_ops!() => {
                        let handled = tryv!(unsafe {
                            (*ctx).exec_literals_vars_op(
                                op, code, &mut ip, base, frame_idx, closure, first_reg,
                            )
                        });
                        debug_assert!(handled, "exec_literals_vars_op must handle grouped opcodes");
                    }

                    math_cmp_ops!() => {
                        let handled = tryv!(unsafe {
                            (*ctx).exec_math_cmp_op(
                                op, code, &mut ip, base, frame_idx, closure, first_reg,
                            )
                        });
                        debug_assert!(handled, "exec_math_cmp_op must handle grouped opcodes");
                    }

                    control_call_ops!() => {
                        if let Some(flow) = tryv!(unsafe {
                            (*ctx).exec_control_calls_op(
                                op, code, &mut ip, base, frame_idx, closure, first_reg, depth,
                            )
                        }) {
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

                    object_ops!() => {
                        if let Some(flow) = tryv!(unsafe {
                            (*ctx).exec_objects_collections_op(
                                op, code, &mut ip, base, frame_idx, closure, first_reg,
                            )
                        }) {
                            match flow {
                                crate::exec::dispatch::ops_objects_collections::ObjectFlow::ContinueInstruction => {}
                                crate::exec::dispatch::ops_objects_collections::ObjectFlow::ContinueFrame => {
                                    continue 'frame_loop;
                                }
                            }
                        }
                    }

                    class_ops!() => {
                        unsafe { (*ctx).frames[frame_idx].ip = ip };
                        tryv!(unsafe {
                            (*ctx).exec_class_op(
                                op, code, &mut ip, base, frame_idx, closure, first_reg,
                            )
                        });
                        let frame_idx2 = unsafe { (*ctx).frames.len() } - 1;
                        ip = unsafe { (*ctx).frames[frame_idx2].ip };
                    }
                    OpCode::MakeEnumVariant => {
                        unsafe { (*ctx).frames[frame_idx].ip = ip };
                        tryv!(unsafe {
                            (*ctx)
                                .exec_make_enum_variant_reg(code, &mut ip, base, frame_idx, closure)
                        });
                        let frame_idx2 = unsafe { (*ctx).frames.len() } - 1;
                        ip = unsafe { (*ctx).frames[frame_idx2].ip };
                    }

                    OpCode::Throw => {
                        let w1 = code[ip];
                        let src = hi(w1);
                        let val = reg_box!(src);
                        unsafe { (*ctx).frames[frame_idx].ip = ip };
                        match unsafe { crate::exec::exceptions::raise(ctx, val, depth) } {
                            Ok(()) => continue 'frame_loop,
                            Err(err) => return Err(err),
                        }
                    }

                    OpCode::Yield => {
                        let w1 = code[ip];
                        ip += 1;
                        let dest = hi(w1) as u8;
                        let src = lo(w1);
                        let val = reg_box!(src);
                        unsafe { (*ctx).frames[frame_idx].ip = ip };
                        unsafe {
                            (*ctx).vm_suspend = Some(crate::exec::VmSuspend::Yield {
                                value: val,
                                dest_reg: dest,
                            })
                        };
                        return Ok(VmValue::null());
                    }
                    OpCode::Await => {
                        let src = hi(code[ip]);
                        ip += 1;
                        let fut = reg_box!(src);
                        unsafe { (*ctx).frames[frame_idx].ip = ip };
                        match tryv!(unsafe { (*ctx).begin_await(fut, first_reg as u16) }) {
                            crate::exec::ctx_tasks::AwaitStep::Ready(v) => {
                                tryv!(unsafe { (*ctx).stack.unbox_into_reg(base, first_reg, v) });
                            }
                            crate::exec::ctx_tasks::AwaitStep::Thrown(val) => {
                                match unsafe { crate::exec::exceptions::raise(ctx, val, depth) } {
                                    Ok(()) => continue 'frame_loop,
                                    Err(err) => return Err(err),
                                }
                            }
                            crate::exec::ctx_tasks::AwaitStep::Entered => continue 'frame_loop,
                            crate::exec::ctx_tasks::AwaitStep::Pending(cell) => {
                                unsafe {
                                    (*ctx).vm_suspend = Some(crate::exec::VmSuspend::Await {
                                        cell,
                                        dest_reg: first_reg as u16,
                                    })
                                };
                                return Ok(VmValue::null());
                            }
                        }
                    }

                    OpCode::LoadModule | OpCode::LoadModuleSlot | OpCode::StoreModuleSlot => {
                        unsafe { (*ctx).frames[frame_idx].ip = ip };
                        tryv!(unsafe {
                            (*ctx).exec_module_op_reg(
                                op, code, &mut ip, frame_idx, closure, first_reg,
                            )
                        });
                        ip = unsafe { (*ctx).frames[frame_idx].ip };
                    }

                    OpCode::InvokeRuntimeStatic => {
                        unsafe { (*ctx).frames[frame_idx].ip = ip };
                        tryv!(unsafe {
                            (*ctx).exec_invoke_runtime_static_reg(
                                code, &mut ip, base, frame_idx, closure,
                            )
                        });
                        let frame_idx2 = unsafe { (*ctx).frames.len() } - 1;
                        ip = unsafe { (*ctx).frames[frame_idx2].ip };
                    }
                    OpCode::Intrinsic | OpCode::IntrinsicDirect | OpCode::CallNativeOp => {
                        tryv!(unsafe {
                            (*ctx).exec_native_op(op, code, &mut ip, frame_idx, first_reg, closure)
                        });
                    }

                    OpCode::Try
                    | OpCode::PopTry
                    | OpCode::GetEnumTag
                    | OpCode::Spawn
                    | OpCode::LoadStaticFn => {
                        tryv!(unsafe {
                            (*ctx).exec_misc_op(op, code, &mut ip, base, first_reg, closure)
                        });
                    }
                    OpCode::Nop => {}
                }

                if unsafe { (*ctx).vm_suspend.is_some() } {
                    unsafe { (*ctx).frames[frame_idx].ip = ip };
                    return Ok(VmValue::null());
                }
            }
        }
        Ok(VmValue::null())
    }
}
