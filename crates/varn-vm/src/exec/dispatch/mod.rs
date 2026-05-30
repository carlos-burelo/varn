use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_core::OpCode;

pub mod collections;
pub mod control;
pub mod exceptions;
mod literals;
pub mod math;
pub mod modules;
pub mod objects;
pub mod reg_ops;
pub mod variables;

pub mod ops_control_calls;
pub mod ops_literals_vars;
pub mod ops_math_cmp;
pub mod ops_objects_collections;

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
        'frame_loop: while self.frames.len() > depth {
            let frame_idx = self.frames.len() - 1;

            let closure_ptr: *const crate::frame::VmClosure = &*self.frames[frame_idx].closure;
            let closure = unsafe { &*closure_ptr };

            let is_first_entry = self.frames[frame_idx].ip == 0;

            if !self.no_jit && closure.jit_entry.is_some() {
                let jit_fn = closure.jit_entry.unwrap();
                if is_first_entry {
                    varn_jit::JIT_STATS
                        .jit_runs
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                if self.trace {
                    self.trace_event("JIT ENTRY", frame_idx, closure, 0, None);
                }
                let res = unsafe {
                    (jit_fn)(
                        self.stack.as_mut_ptr() as *mut std::ffi::c_void,
                        closure_ptr as *const std::ffi::c_void,
                        self.frames[frame_idx].base,
                        self as *mut ExecCtx as *mut std::ffi::c_void,
                    )
                };
                if self.trace {
                    self.trace_event("JIT EXIT", frame_idx, closure, 0, None);
                }

                let frame = self.frames.pop().unwrap();
                self.record_frame_pop();
                self.close_upvalues_above(frame.base);
                self.stack.truncate(frame.base);

                let ctor_pos = if !self.pending_constructors.is_empty() {
                    self.pending_constructors
                        .iter()
                        .rposition(|(idx, _)| *idx == frame_idx)
                } else {
                    None
                };

                let final_val = if let Some(pos) = ctor_pos {
                    let (_, instance_nv) = self.pending_constructors.remove(pos);
                    if res.is_null() {
                        instance_nv
                    } else {
                        res
                    }
                } else {
                    res
                };

                if let Some(return_reg) = frame.return_reg {
                    let caller_base = self.frames.last().map(|f| f.base).unwrap_or(0);
                    self.stack[caller_base + return_reg as usize] = final_val;
                }

                if self.frames.len() == depth {
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

            let base = self.frames[frame_idx].base;
            let mut ip = self.frames[frame_idx].ip;
            let code_len = closure.proto.chunk.code.len();

            loop {
                if ip >= code_len {
                    self.frames.last_mut().unwrap().ip = ip;
                    let res = self.reg_return(base, 0);
                    if self.frames.len() == depth {
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

                #[cfg(feature = "profiling")]
                if let Some(counts) = self.opcode_counts.as_ref() {
                    counts[(raw_op as u8) as usize]
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }

                macro_rules! reg {
                    ($r:expr) => {
                        *({
                            #[cfg(debug_assertions)]
                            {
                                let idx = base + $r;
                                if idx >= self.stack.len() {
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
                                        self.stack.len()
                                    );
                                }
                            }
                            &mut self.stack[base + $r]
                        })
                    };
                }

                match op {
                    OpCode::LoadGlobal
                    | OpCode::LoadGlobalIdx
                    | OpCode::StoreGlobal
                    | OpCode::StoreGlobalIdx
                    | OpCode::DefineGlobal
                    | OpCode::DefineGlobalIdx
                    | OpCode::LoadUpvalue
                    | OpCode::StoreUpvalue
                    | OpCode::CloseUpvalue
                    | OpCode::LoadNull
                    | OpCode::LoadTrue
                    | OpCode::LoadFalse
                    | OpCode::LoadInt
                    | OpCode::LoadIntZero
                    | OpCode::LoadIntOne
                    | OpCode::LoadIntMinusOne
                    | OpCode::LoadConst
                    | OpCode::Move => {
                        let handled = self.exec_literals_vars_op(
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
                        let handled = self.exec_math_cmp_op(
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
                    | OpCode::CallMethod
                    | OpCode::InvokeVirtual
                    | OpCode::CallSpread => {
                        if let Some(flow) = self.exec_control_calls_op(
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
                        if let Some(flow) = self.exec_objects_collections_op(
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
                        self.frames[frame_idx].ip = ip;
                        self.exec_class_op(
                            op, code, &mut ip, base, frame_idx, &closure, first_reg,
                        )?;
                        let frame_idx2 = self.frames.len() - 1;
                        ip = self.frames[frame_idx2].ip;
                    }
                    OpCode::MakeEnumVariant => {
                        self.frames[frame_idx].ip = ip;
                        self.exec_make_enum_variant_reg(code, &mut ip, base, frame_idx, &closure)?;
                        let frame_idx2 = self.frames.len() - 1;
                        ip = self.frames[frame_idx2].ip;
                    }
                    OpCode::GetEnumTag => {
                        let src = hi(code[ip]);
                        ip += 1;
                        let v = reg![src];
                        reg![first_reg] = self.exec_get_enum_tag(v)?;
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
                            &mut self.try_handlers,
                            catch_ip,
                            self.frames.len(),
                            err_reg,
                        );
                    }
                    OpCode::PopTry => {
                        crate::exec::exceptions::pop_try(&mut self.try_handlers);
                    }
                    OpCode::Throw => {
                        let w1 = code[ip];
                        let src = hi(w1);
                        let val = reg![src];
                        let err = crate::exec::exceptions::build_thrown_error(
                            val,
                            &self.heap,
                            &self.frames,
                        );
                        if let Some(handler) = self.try_handlers.pop() {
                            while self.frames.len() > handler.frame_depth {
                                self.record_frame_pop();
                                let f = self.frames.pop().unwrap();
                                self.close_upvalues_above(f.base);
                            }

                            let f2 = self.frames.len() - 1;
                            let b2 = self.frames[f2].base;
                            let required_depth =
                                b2 + self.frames[f2].closure.proto.register_count as usize;
                            self.stack.truncate(required_depth);
                            let thrown_val = err.thrown.unwrap_or(VmValue::null());

                            let slot = b2 + handler.err_reg as usize;
                            if slot < self.stack.len() {
                                self.stack[slot] = thrown_val;
                            } else {
                                self.stack.resize(slot + 1, VmValue::null());
                                self.stack[slot] = thrown_val;
                            }
                            let new_frame_idx = self.frames.len() - 1;
                            self.frames[new_frame_idx].ip = handler.catch_ip;
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
                        self.frames[frame_idx].ip = ip;
                        self.vm_suspend = Some(crate::exec::VmSuspend::Yield {
                            value: val,
                            dest_reg: dest,
                        });
                        return Ok(VmValue::null());
                    }
                    OpCode::Await => {
                        let src = hi(code[ip]);
                        ip += 1;
                        let fut = reg![src];
                        self.frames[frame_idx].ip = ip;
                        self.vm_suspend = Some(crate::exec::VmSuspend::Await {
                            value: self.heap.extract(fut),
                            dest_reg: first_reg as u16,
                        });
                        return Ok(VmValue::null());
                    }
                    OpCode::Spawn => {
                        let w1 = code[ip];
                        ip += 1;
                        let (dest, src) = (hi(w1), lo(w1));
                        let task_val = reg![src];
                        reg![dest] = self.exec_spawn(task_val)?;
                    }

                    OpCode::LoadModule | OpCode::LoadModuleSlot | OpCode::StoreModuleSlot => {
                        self.frames[frame_idx].ip = ip;
                        self.exec_module_op_reg(
                            op, code, &mut ip, base, frame_idx, &closure, first_reg,
                        )?;
                        ip = self.frames[frame_idx].ip;
                    }

                    OpCode::InvokeRuntimeStatic => {
                        self.frames[frame_idx].ip = ip;
                        self.exec_invoke_runtime_static_reg(
                            code, &mut ip, base, frame_idx, &closure,
                        )?;
                        let frame_idx2 = self.frames.len() - 1;
                        ip = self.frames[frame_idx2].ip;
                    }
                    OpCode::Nop => {}
                }

                if self.vm_suspend.is_some() {
                    self.frames[frame_idx].ip = ip;
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

        let is_module_frame = frame.closure.proto.name.as_deref() == Some("<module>")
            && !frame.closure.proto.chunk.source_file.is_empty();

        self.stack.truncate(frame.base);

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
            let source_file = frame.closure.proto.chunk.source_file.to_string();
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
            OpCode::Pow => Ok(arith::pow(a, b)),
            OpCode::BitAnd => Ok(VmValue::from_int(a.as_int() & b.as_int())),
            OpCode::BitOr => Ok(VmValue::from_int(a.as_int() | b.as_int())),
            OpCode::BitXor => Ok(VmValue::from_int(a.as_int() ^ b.as_int())),
            OpCode::Shl => Ok(arith::shl(a, b)),
            OpCode::Shr => Ok(arith::shr(a, b)),
            OpCode::Ushr => Ok(arith::ushr(a, b)),
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
        if v.is_null() {
            return "null";
        }
        if v.is_int() {
            return "int";
        }
        if v.is_f64() {
            return "float";
        }
        if v.is_bool() {
            return "bool";
        }
        if !v.is_heap() {
            return "unknown";
        }
        match self.heap.get(v.as_heap_idx()) {
            Some(crate::heap::HeapObj::Str(_)) => "str",
            Some(crate::heap::HeapObj::Array(_)) => "array",
            Some(crate::heap::HeapObj::Object(_)) => "object",
            Some(crate::heap::HeapObj::NativeFn(..)) => "function",
            Some(crate::heap::HeapObj::VmClosure(_)) => "function",
            Some(crate::heap::HeapObj::Class(_)) => "class",
            Some(crate::heap::HeapObj::Char(_)) => "char",
            Some(crate::heap::HeapObj::BigInt(_)) => "bigint",
            Some(crate::heap::HeapObj::Decimal(_)) => "decimal",
            Some(crate::heap::HeapObj::Symbol(_)) => "symbol",
            _ => "unknown",
        }
    }
}

fn frames_return_reg(frames: &[crate::frame::CallFrame]) -> Option<u16> {
    frames.last()?.return_reg
}
