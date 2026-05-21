use crate::error::VmResult;
use crate::value::VmValue;
use std::cell::RefCell;
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

#[derive(Debug, PartialEq, Eq)]
pub enum ControlSignal {
    ContinueInstruction,
    ContinueFrame,
    Return(VmValue),
    None,
}

use crate::exec::ctx::ExecCtx;

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

    fn run_until_inner(&mut self, depth: usize) -> VmResult<VmValue> {
        'frame_loop: while self.frames.len() > depth {
            let frame_idx = self.frames.len() - 1;

            // SAFETY: `closure` lives inside `self.frames[frame_idx]` which stays
            // alive for the entire inner loop. We never push/pop frames inside the
            // raw-pointer section without breaking to 'frame_loop first, so the
            // referent is stable. Using a raw pointer avoids an Rc refcount bump on
            // every frame entry (hot in recursive / iterative code).
            let closure_ptr: *const crate::frame::VmClosure =
                &*self.frames[frame_idx].closure;
            let closure = unsafe { &*closure_ptr };
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
                        self.stack[base + $r]
                    };
                }

                match op {
                    OpCode::LoadNull => {
                        reg![first_reg] = VmValue::null();
                    }
                    OpCode::LoadTrue => {
                        reg![first_reg] = VmValue::bool_true();
                    }
                    OpCode::LoadFalse => {
                        reg![first_reg] = VmValue::bool_false();
                    }
                    OpCode::LoadInt => {
                        let val = code[ip] as i16;
                        ip += 1;
                        reg![first_reg] = VmValue::from_int(val as i64);
                    }
                    OpCode::LoadIntZero => {
                        reg![first_reg] = VmValue::from_int(0);
                    }
                    OpCode::LoadIntOne => {
                        reg![first_reg] = VmValue::from_int(1);
                    }
                    OpCode::LoadIntMinusOne => {
                        reg![first_reg] = VmValue::from_int(-1);
                    }
                    OpCode::LoadConst => {
                        let cidx = code[ip] as usize;
                        ip += 1;
                        let nv = closure.constants[cidx];
                        reg![first_reg] = nv;
                    }

                    OpCode::Move => {
                        let w1 = code[ip];
                        ip += 1;
                        reg![first_reg] = reg![hi(w1)];
                    }

                    OpCode::LoadGlobal
                    | OpCode::StoreGlobal
                    | OpCode::DefineGlobal
                    | OpCode::LoadGlobalIdx
                    | OpCode::StoreGlobalIdx
                    | OpCode::DefineGlobalIdx => {
                        self.frames[frame_idx].ip = ip;
                        self.exec_variable_op(
                            op, code, &mut ip, base, frame_idx, &closure, first_reg,
                        )?;
                    }

                    OpCode::LoadUpvalue => {
                        let w1 = code[ip];
                        ip += 1;
                        let (dest, uv) = (hi(w1), lo(w1));
                        reg![dest] = closure.upvalues[uv].read(&self.stack);
                    }
                    OpCode::StoreUpvalue => {
                        let w1 = code[ip];
                        ip += 1;
                        let (uv, src) = (hi(w1), lo(w1));
                        let val = reg![src];
                        closure.upvalues[uv].write(val, &mut self.stack);
                    }
                    OpCode::CloseUpvalue => {
                        let w1 = code[ip];
                        ip += 1;
                        let lowest = hi(w1);
                        self.close_upvalues_above(base + lowest);
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
                    | OpCode::Ushr => {
                        let w1 = code[ip];
                        ip += 1;
                        let (src1, src2) = (hi(w1), lo(w1));
                        let a = reg![src1];
                        let b = reg![src2];
                        let r = self.exec_arith(op, a, b)?;
                        reg![first_reg] = r;
                    }
                    OpCode::Negate => {
                        let src = hi(code[ip]);
                        ip += 1;
                        let v = reg![src];
                        reg![first_reg] = crate::exec::arith::negate(v, &mut self.heap);
                    }
                    OpCode::Not => {
                        let src = hi(code[ip]);
                        ip += 1;
                        let v = reg![src];
                        reg![first_reg] = crate::exec::compare::logical_not(v);
                    }

                    OpCode::AddImm => {
                        let w1 = code[ip];
                        ip += 1;
                        let src = hi(w1) as usize;
                        let imm = lo(w1) as i8 as i64;
                        let v = reg![src];
                        if v.is_int() {
                            let (r, overflow) = v.as_int().overflowing_add(imm);
                            reg![first_reg] = if overflow {
                                VmValue::from_f64(v.as_int() as f64 + imm as f64)
                            } else {
                                VmValue::from_int(r)
                            };
                        } else {
                            reg![first_reg] = crate::exec::arith::add(v, VmValue::from_int(imm), &mut self.heap)?;
                        }
                    }

                    OpCode::SubImm => {
                        let w1 = code[ip];
                        ip += 1;
                        let src = hi(w1) as usize;
                        let imm = lo(w1) as i8 as i64;
                        let v = reg![src];
                        if v.is_int() {
                            let (r, overflow) = v.as_int().overflowing_sub(imm);
                            reg![first_reg] = if overflow {
                                VmValue::from_f64(v.as_int() as f64 - imm as f64)
                            } else {
                                VmValue::from_int(r)
                            };
                        } else {
                            reg![first_reg] = crate::exec::arith::sub(v, VmValue::from_int(imm), &mut self.heap)?;
                        }
                    }

                    OpCode::Eq
                    | OpCode::Neq
                    | OpCode::Lt
                    | OpCode::Lte
                    | OpCode::Gt
                    | OpCode::Gte => {
                        let w1 = code[ip];
                        ip += 1;
                        let (src1, src2) = (hi(w1), lo(w1));
                        let a = reg![src1];
                        let b = reg![src2];
                        let r = self.exec_cmp(op, a, b);
                        reg![first_reg] = r;
                    }

                    OpCode::ToString => {
                        let src = hi(code[ip]);
                        ip += 1;
                        let v = reg![src];
                        let s = self.heap.str_repr(v);
                        reg![first_reg] = self.heap.alloc_str(s);
                    }
                    OpCode::StrConcat => {
                        let w1 = code[ip];
                        ip += 1;
                        let (src1, src2) = (hi(w1), lo(w1));
                        let a = reg![src1];
                        let b = reg![src2];
                        let sa = self.heap.str_repr(a);
                        let sb = self.heap.str_repr(b);
                        let combined = format!("{sa}{sb}");
                        reg![first_reg] = self.heap.alloc_str(combined);
                    }

                    OpCode::BuildStr => {
                        let count = hi(code[ip]) as usize;
                        ip += 1;
                        // Gather part strings, compute total capacity, concat in one alloc.
                        let parts: Vec<String> = (0..count)
                            .map(|i| {
                                let reg_idx = hi(code[ip + i]) as usize;
                                self.heap.str_repr(reg![reg_idx])
                            })
                            .collect();
                        ip += count;
                        let total_len: usize = parts.iter().map(|s| s.len()).sum();
                        let mut combined = String::with_capacity(total_len);
                        for p in &parts {
                            combined.push_str(p);
                        }
                        reg![first_reg] = self.heap.alloc_str(combined);
                    }

                    OpCode::StrLength => {
                        let src = hi(code[ip]);
                        ip += 1;
                        let v = reg![src];
                        let len = self.exec_str_length(v)?;
                        reg![first_reg] = len;
                    }
                    OpCode::StrSlice => {
                        let w1 = code[ip];
                        ip += 1;
                        let (src1, src2) = (hi(w1), lo(w1));
                        let s = reg![src1];
                        let idx = reg![src2];
                        reg![first_reg] = self.exec_str_slice(s, idx)?;
                    }

                    OpCode::Jump => {
                        let offset = ((code[ip] as u32) << 16 | code[ip + 1] as u32) as usize;
                        ip += 2;
                        ip += offset;
                    }
                    OpCode::Loop => {
                        let offset = ((code[ip] as u32) << 16 | code[ip + 1] as u32) as usize;
                        ip += 2;
                        ip -= offset;
                    }
                    OpCode::JumpIfFalse => {
                        let offset = ((code[ip] as u32) << 16 | code[ip + 1] as u32) as usize;
                        ip += 2;
                        if !reg![first_reg].is_truthy() {
                            ip += offset;
                        }
                    }
                    OpCode::JumpIfTrue => {
                        let offset = ((code[ip] as u32) << 16 | code[ip + 1] as u32) as usize;
                        ip += 2;
                        if reg![first_reg].is_truthy() {
                            ip += offset;
                        }
                    }

                    OpCode::Return => {
                        let w1 = code[ip];
                        let src = lo(w1);
                        let val = reg![src];
                        let res = self.reg_return(base, src);
                        if self.frames.len() == depth {
                            return Ok(res);
                        }
                        let _ = val;
                        continue 'frame_loop;
                    }

                    OpCode::Call => {
                        let w1 = code[ip];
                        ip += 1;
                        let w2 = code[ip];
                        ip += 1;
                        let (dest, callee_reg) = (hi(w1), lo(w1));
                        let (arg_count, arg_start) = (hi(w2), lo(w2));
                        self.frames[frame_idx].ip = ip;
                        let callee = reg![callee_reg];
                        let result = self
                            .exec_call_reg(callee, base, arg_start, arg_count, dest, frame_idx)?;
                        if result {
                            continue 'frame_loop;
                        }

                        // exec_call_reg returned false: call resolved inline (native/bound),
                        // no new frame was pushed. Re-fetch frame_idx in case frames shifted.
                        let frame_idx2 = self.frames.len() - 1;
                        ip = self.frames[frame_idx2].ip;
                    }

                    OpCode::CallMethod => {
                        let cs = first_reg;
                        let w1 = code[ip];
                        ip += 1;
                        let name_idx = code[ip] as usize;
                        ip += 1;
                        let w3 = code[ip];
                        ip += 1;
                        let (dest, obj_reg) = (hi(w1), lo(w1));
                        let (arg_count, arg_start) = (hi(w3), lo(w3));
                        self.frames[frame_idx].ip = ip;
                        let this_val = reg![obj_reg];
                        let jumped = self.exec_call_method_reg(
                            this_val, base, name_idx, cs, arg_start, arg_count, dest, frame_idx,
                            &closure,
                        )?;
                        if jumped {
                            continue 'frame_loop;
                        }
                        let frame_idx2 = self.frames.len() - 1;
                        ip = self.frames[frame_idx2].ip;
                    }

                    OpCode::InvokeVirtual => {
                        let w1 = code[ip];
                        ip += 1;
                        let name_idx = code[ip] as usize;
                        ip += 1;
                        let w3 = code[ip];
                        ip += 1;
                        let (dest, this_reg) = (hi(w1), lo(w1));
                        let (arg_count, arg_start) = (hi(w3), lo(w3));
                        self.frames[frame_idx].ip = ip;
                        let this_val = reg![this_reg];
                        let method_name_nv = closure.constants[name_idx];
                        let method_name = self
                            .heap
                            .str_val(method_name_nv)
                            .expect("InvokeVirtual: not a string const");
                        let method_nv = crate::exec::props::get_property(
                            this_val,
                            &method_name,
                            &mut self.heap,
                        )?;
                        let jumped = self.exec_call_reg(
                            method_nv, base, arg_start, arg_count, dest, frame_idx,
                        )?;
                        if jumped {
                            continue 'frame_loop;
                        }
                        let frame_idx2 = self.frames.len() - 1;
                        ip = self.frames[frame_idx2].ip;
                    }

                    OpCode::CallSpread => {
                        let w1 = code[ip];
                        ip += 1;
                        let w2 = code[ip];
                        ip += 1;
                        let (dest, callee_reg) = (hi(w1), lo(w1));
                        let (arg_count, arg_start) = (hi(w2), lo(w2));
                        self.frames[frame_idx].ip = ip;
                        let callee = reg![callee_reg];
                        let jumped = self.exec_call_spread_reg(
                            callee, base, arg_start, arg_count, dest, frame_idx,
                        )?;
                        if jumped {
                            continue 'frame_loop;
                        }
                        let frame_idx2 = self.frames.len() - 1;
                        ip = self.frames[frame_idx2].ip;
                    }

                    OpCode::MakeClosure => {
                        let w1 = code[ip];
                        ip += 1;
                        let proto_idx = code[ip] as usize;
                        ip += 1;
                        let (dest, uv_count) = (hi(w1), lo(w1));
                        let proto = match closure.proto.chunk.constants.get(proto_idx) {
                            Some(varn_types::PoolEntry::Function(p)) => p.clone(),
                            _ => {
                                return Err(crate::error::RuntimeError::new(format!(
                                    "MakeClosure: const {proto_idx} is not a function"
                                )))
                            }
                        };
                        let mut upvalues = Vec::with_capacity(uv_count);
                        for _ in 0..uv_count {
                            let uv_desc = code[ip];
                            ip += 1;
                            let is_local = hi(uv_desc) != 0;
                            let index = lo(uv_desc);
                            if is_local {
                                upvalues.push(self.capture_upvalue(base + index));
                            } else {
                                upvalues.push(closure.upvalues[index].clone());
                            }
                        }
                        let proto_ptr = std::rc::Rc::as_ptr(&proto) as usize;
                        let constants = self
                            .proto_constants
                            .entry(proto_ptr)
                            .or_insert_with(|| {
                                std::rc::Rc::new(crate::exec::calls::resolve_constants(
                                    &proto,
                                    &mut self.heap,
                                ))
                            })
                            .clone();
                        let ic_cache = self
                            .proto_ic_caches
                            .entry(proto_ptr)
                            .or_insert_with(|| {
                                std::rc::Rc::new(std::cell::RefCell::new(
                                    (0..proto.cache_count)
                                        .map(|_| varn_types::chunk::PolyICSlot::new())
                                        .collect::<Vec<_>>(),
                                ))
                            })
                            .clone();
                        let new_feedback = self
                            .proto_feedback
                            .entry(proto_ptr)
                            .or_insert_with(|| {
                                std::rc::Rc::new(std::cell::RefCell::new(
                                    varn_types::chunk::FeedbackVector::new(proto.cache_count),
                                ))
                            })
                            .clone();
                        let vm_closure = std::rc::Rc::new(crate::frame::VmClosure::with_upvalues(
                            proto,
                            upvalues,
                            constants,
                            ic_cache,
                            new_feedback,
                        ));
                        reg![dest] = self.heap.alloc_vm_closure(vm_closure);
                    }

                    OpCode::GetProperty => {
                        let w1 = code[ip];
                        ip += 1;
                        let obj_reg = hi(w1);
                        let cs_idx = lo(w1);
                        let name_idx = code[ip] as usize;
                        ip += 1;
                        self.frames[frame_idx].ip = ip;
                        let obj = reg![obj_reg];
                        let jumped = self.exec_get_property_reg(
                            obj, name_idx, cs_idx, first_reg, base, frame_idx, &closure,
                        )?;
                        if jumped {
                            continue 'frame_loop;
                        }
                        let frame_idx2 = self.frames.len() - 1;
                        ip = self.frames[frame_idx2].ip;
                    }
                    OpCode::GetPropertyMaybe => {
                        let obj_reg = hi(code[ip]);
                        ip += 1;
                        let name_idx = code[ip] as usize;
                        ip += 1;
                        let obj = reg![obj_reg];
                        let name_nv = closure.constants[name_idx];
                        let name = self.heap.str_val(name_nv).unwrap_or_else(|| {
                            closure.proto.chunk.constants[name_idx]
                                .as_str()
                                .unwrap_or("")
                                .into()
                        });
                        let result =
                            crate::exec::props::get_property_maybe(obj, &name, &mut self.heap);
                        reg![first_reg] = result;
                    }
                    OpCode::SetProperty => {
                        let w1 = code[ip];
                        ip += 1;
                        let val_reg = hi(w1);
                        let cs_idx = lo(w1);
                        let name_idx = code[ip] as usize;
                        ip += 1;
                        let obj_reg = first_reg;
                        self.frames[frame_idx].ip = ip;
                        let obj = reg![obj_reg];
                        let val = reg![val_reg];
                        let jumped = self.exec_set_property_reg(
                            obj, val, name_idx, cs_idx, base, frame_idx, &closure,
                        )?;
                        if jumped {
                            continue 'frame_loop;
                        }
                        let frame_idx2 = self.frames.len() - 1;
                        ip = self.frames[frame_idx2].ip;
                    }
                    OpCode::GetFixedField => {
                        let obj_reg = hi(code[ip]);
                        ip += 1;
                        let slot = code[ip] as usize;
                        ip += 1;
                        let obj = reg![obj_reg];
                        reg![first_reg] = self.exec_get_fixed_field(obj, slot)?;
                    }
                    OpCode::SetFixedField => {
                        let val_reg = hi(code[ip]);
                        ip += 1;
                        let slot = code[ip] as usize;
                        ip += 1;
                        let obj = reg![first_reg];
                        let val = reg![val_reg];
                        self.exec_set_fixed_field(obj, slot, val)?;
                    }
                    OpCode::GetSuper => {
                        let name_idx = code[ip] as usize;
                        ip += 1;

                        let this_val = self.stack[base];
                        self.frames[frame_idx].ip = ip;
                        let val =
                            self.exec_get_super_reg(this_val, name_idx, frame_idx, &closure)?;
                        let frame_idx2 = self.frames.len() - 1;
                        self.stack[base + first_reg] = val;
                        ip = self.frames[frame_idx2].ip;
                    }
                    OpCode::GetSymbol => {
                        let obj_reg = hi(code[ip]);
                        ip += 1;
                        let sym_idx = code[ip] as usize;
                        ip += 1;
                        let obj = reg![obj_reg];
                        reg![first_reg] = self.exec_get_symbol(obj, sym_idx, &closure)?;
                    }
                    OpCode::AssertNotNull => {
                        let w1 = code[ip];
                        ip += 1;
                        let src = hi(w1);
                        let v = reg![src];
                        self.exec_assert_not_null(v)?;
                    }
                    OpCode::DeclareField => {
                        let w1 = code[ip];
                        ip += 1;
                        let name_idx = code[ip] as usize;
                        ip += 1;
                        let obj_reg = hi(w1);
                        let obj = reg![obj_reg];
                        self.exec_declare_field(obj, name_idx, frame_idx, &closure)?;
                    }

                    OpCode::GetIndex => {
                        let w1 = code[ip];
                        ip += 1;
                        let obj_reg = hi(w1);
                        let idx_reg = lo(w1);
                        let obj = reg![obj_reg];
                        let key_nv = reg![idx_reg];
                        let result = self.exec_get_index_nv(obj, key_nv)?;
                        reg![first_reg] = result;
                    }
                    OpCode::SetIndex => {
                        let w1 = code[ip];
                        ip += 1;
                        let idx_reg = hi(w1);
                        let val_reg = lo(w1);
                        let obj = reg![first_reg];
                        let idx = reg![idx_reg];
                        let val = reg![val_reg];
                        self.exec_set_index(obj, idx, val)?;
                    }
                    OpCode::BuildArray => {
                        let w1 = code[ip];
                        ip += 1;
                        let w2 = code[ip];
                        ip += 1;
                        let (dest, start_reg) = (hi(w1), lo(w1));
                        let count = hi(w2);
                        let mut elems = Vec::with_capacity(count);
                        for i in 0..count {
                            let nv = self.stack[base + start_reg + i];
                            elems.push(self.heap.extract(nv));
                        }
                        reg![dest] = self.heap.alloc_array(elems);
                    }
                    OpCode::BuildObject => {
                        let w1 = code[ip];
                        ip += 1;
                        let (dest, count) = (hi(w1), lo(w1));
                        let obj_nv = self.heap.alloc_object();
                        for _ in 0..count {
                            let k_idx = code[ip] as usize;
                            ip += 1;
                            let w = code[ip];
                            ip += 1;
                            let val_reg = hi(w);
                            let key_nv = closure.constants[k_idx];
                            let key = self
                                .heap
                                .str_val(key_nv)
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| {
                                    closure.proto.chunk.constants[k_idx]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_string()
                                });
                            let val = reg![val_reg];
                            crate::exec::props::set_property(obj_nv, &key, val, &mut self.heap)?;
                        }
                        reg![dest] = obj_nv;
                    }
                    OpCode::BuildObjectWithShape => {
                        let w1 = code[ip];
                        ip += 1;
                        let shape_idx = code[ip] as usize;
                        ip += 1;
                        let (dest, start_reg) = (hi(w1), lo(w1));
                        reg![dest] = self
                            .exec_build_object_with_shape(base, start_reg, shape_idx, &closure)?;
                    }
                    OpCode::ObjectRest => {
                        let w1 = code[ip];
                        ip += 1;
                        let w2 = code[ip];
                        ip += 1;
                        let (dest, src) = (hi(w1), lo(w1));
                        let skip_count = hi(w2);
                        let mut skip_keys = Vec::with_capacity(skip_count);
                        for _ in 0..skip_count {
                            let k_idx = code[ip] as usize;
                            ip += 1;
                            let key_nv = closure.constants[k_idx];
                            skip_keys.push(self.heap.str_val(key_nv).unwrap_or_else(|| {
                                closure.proto.chunk.constants[k_idx]
                                    .as_str()
                                    .unwrap_or("")
                                    .into()
                            }));
                        }
                        let obj = reg![src];
                        reg![dest] = self.exec_object_rest(obj, &skip_keys)?;
                    }
                    OpCode::ObjectKeys => {
                        let src = hi(code[ip]);
                        ip += 1;
                        let obj = reg![src];
                        reg![first_reg] = self.exec_object_keys(obj)?;
                    }
                    OpCode::ObjectMerge => {
                        let src = hi(code[ip]);
                        ip += 1;
                        let dest_nv = reg![first_reg];
                        let src_nv = reg![src];
                        reg![first_reg] = crate::exec::collections::object_merge(
                            dest_nv,
                            src_nv,
                            &mut self.heap,
                        )?;
                    }
                    OpCode::WrapSpread => {
                        let src = hi(code[ip]);
                        ip += 1;
                        let v = self.heap.extract(reg![src]);
                        reg![first_reg] = self.heap.intern(varn_types::Value::Spread(Box::new(v)));
                    }
                    OpCode::ArrayLength => {
                        let src = hi(code[ip]);
                        ip += 1;
                        let arr = reg![src];
                        reg![first_reg] = self.exec_array_length(arr)?;
                    }
                    OpCode::ArrayPush => {
                        let val_reg = hi(code[ip]);
                        ip += 1;
                        let arr = reg![first_reg];
                        let val = reg![val_reg];
                        self.exec_array_push(arr, val)?;
                    }
                    OpCode::ArrayPop => {
                        let arr_reg = hi(code[ip]);
                        ip += 1;
                        let arr = reg![arr_reg];
                        reg![first_reg] = self.exec_array_pop(arr)?;
                    }
                    OpCode::ArrayExtend => {
                        let src_reg = hi(code[ip]);
                        ip += 1;
                        let arr = reg![first_reg];
                        let src = reg![src_reg];
                        self.exec_array_extend(arr, src)?;
                    }
                    OpCode::In => {
                        let w1 = code[ip];
                        ip += 1;
                        let (src1, src2) = (hi(w1), lo(w1));
                        let a = reg![src1];
                        let b = reg![src2];
                        let r = crate::exec::advanced::op_in(a, b, &self.heap);
                        reg![first_reg] = VmValue::from_bool(r);
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
                    OpCode::Instanceof => {
                        let w1 = code[ip];
                        ip += 1;
                        let (src1, src2) = (hi(w1), lo(w1));
                        let a = reg![src1];
                        let b = reg![src2];
                        let r = crate::exec::advanced::instanceof(a, b, &self.heap);
                        reg![first_reg] = VmValue::from_bool(r);
                    }

                    OpCode::Typeof => {
                        let src = hi(code[ip]);
                        ip += 1;
                        let v = reg![src];
                        let s = self.exec_typeof(v);
                        reg![first_reg] = self.heap.alloc_str(s);
                    }
                    OpCode::IsNull => {
                        let src = hi(code[ip]);
                        ip += 1;
                        let v = reg![src];
                        reg![first_reg] = VmValue::from_bool(v.is_null());
                    }
                    OpCode::IsArray => {
                        let src = hi(code[ip]);
                        ip += 1;
                        let v = reg![src];
                        let is_arr = if v.is_heap() {
                            matches!(
                                self.heap.get(v.as_heap_idx()),
                                Some(crate::heap::HeapObj::Array(_))
                            )
                        } else {
                            false
                        };
                        reg![first_reg] = VmValue::from_bool(is_arr);
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

                    OpCode::Import | OpCode::Reexport | OpCode::MergeExports => {
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

    fn reg_return(&mut self, base: usize, src: usize) -> VmValue {
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
            self.modules.insert(source_file, cached);
        }

        if let Some(return_reg) = frame.return_reg {
            let caller_base = self.frames.last().map(|f| f.base).unwrap_or(0);
            self.stack[caller_base + return_reg as usize] = final_val;
        }
        final_val
    }

    fn exec_arith(&mut self, op: OpCode, a: VmValue, b: VmValue) -> VmResult<VmValue> {
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

    fn exec_cmp(&mut self, op: OpCode, a: VmValue, b: VmValue) -> VmValue {
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

    fn exec_typeof(&self, v: VmValue) -> &'static str {
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
