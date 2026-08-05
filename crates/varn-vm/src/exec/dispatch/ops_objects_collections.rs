use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::frame::VmClosure;
use crate::value::VmValue;
use varn_core::OpCode;

use super::{hi, lo};

pub(super) enum ObjectFlow {
    ContinueInstruction,
    ContinueFrame,
}

impl ExecCtx {
    #[inline(always)]
    pub(super) fn exec_objects_collections_op(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        frame_idx: usize,
        closure: &VmClosure,
        first_reg: usize,
    ) -> VmResult<Option<ObjectFlow>> {
        match op {
            OpCode::MakeClosure => {
                let w1 = code[*ip];
                *ip += 1;
                let proto_idx = code[*ip] as usize;
                *ip += 1;
                let (dest, uv_count) = (hi(w1), lo(w1));
                let proto = match closure.proto.chunk.constants.get(proto_idx) {
                    Some(varn_types::PoolEntry::Function(p)) => p.clone(),
                    _ => {
                        return Err(crate::error::RuntimeError::new(format!(
                            "MakeClosure: const {proto_idx} is not a function"
                        )))
                    }
                };
                let proto_ptr = std::rc::Rc::as_ptr(&proto) as usize;
                if uv_count == 0 {
                    if let Some(&(_, cached_val)) = self.static_closures.get(&proto_ptr) {
                        self.stack[base + dest] = cached_val;
                        return Ok(Some(ObjectFlow::ContinueInstruction));
                    }
                }
                let mut upvalues = Vec::with_capacity(uv_count);
                for _ in 0..uv_count {
                    let uv_desc = code[*ip];
                    *ip += 1;
                    let is_local = hi(uv_desc) != 0;
                    let index = lo(uv_desc);
                    if is_local {
                        upvalues.push(self.capture_upvalue(base + index));
                    } else {
                        upvalues.push(closure.upvalues[index].clone());
                    }
                }
                let constants = self
                    .proto_constants
                    .entry(proto_ptr)
                    .or_insert_with(|| {
                        let resolved = std::rc::Rc::new(crate::exec::calls::resolve_constants(
                            &proto,
                            &mut self.heap,
                        ));
                        (proto.clone(), resolved)
                    })
                    .1
                    .clone();
                let vm_closure = std::rc::Rc::new(crate::frame::VmClosure::with_upvalues(
                    proto.clone(),
                    upvalues,
                    constants,
                    self.settings,
                ));
                let val = self.heap.alloc_vm_closure(vm_closure);
                if uv_count == 0 {
                    self.static_closures.insert(proto_ptr, (proto, val));
                }
                self.stack[base + dest] = val;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::GetProperty => {
                let w1 = code[*ip];
                *ip += 1;
                let obj_reg = hi(w1);
                let cs_idx = lo(w1);
                let name_idx = code[*ip] as usize;
                *ip += 1;
                self.frames[frame_idx].ip = *ip;
                let obj = self.stack[base + obj_reg];
                let jumped = self.exec_get_property_reg(
                    obj, name_idx, cs_idx, first_reg, base, frame_idx, closure,
                )?;
                if jumped {
                    return Ok(Some(ObjectFlow::ContinueFrame));
                }
                let frame_idx2 = self.frames.len() - 1;
                *ip = self.frames[frame_idx2].ip;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::GetPropertyMaybe => {
                let obj_reg = hi(code[*ip]);
                *ip += 1;
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let obj = self.stack[base + obj_reg];
                let name_nv = closure.constants[name_idx];
                let name = self.heap.str_val(name_nv).unwrap_or_else(|| {
                    closure.proto.chunk.constants[name_idx]
                        .as_str()
                        .unwrap_or("")
                        .into()
                });
                let result = crate::exec::props::get_property_maybe(obj, &name, &mut self.heap);
                self.stack[base + first_reg] = result;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::SetProperty => {
                let w1 = code[*ip];
                *ip += 1;
                let val_reg = hi(w1);
                let cs_idx = lo(w1);
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let obj_reg = first_reg;
                self.frames[frame_idx].ip = *ip;
                let obj = self.stack[base + obj_reg];
                let val = self.stack[base + val_reg];
                let jumped = self
                    .exec_set_property_reg(obj, val, name_idx, cs_idx, base, frame_idx, closure)?;
                if jumped {
                    return Ok(Some(ObjectFlow::ContinueFrame));
                }
                let frame_idx2 = self.frames.len() - 1;
                *ip = self.frames[frame_idx2].ip;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::GetFixedField => {
                let obj_reg = hi(code[*ip]);
                *ip += 1;
                let slot = code[*ip] as usize;
                *ip += 1;
                let obj = self.stack[base + obj_reg];
                self.stack[base + first_reg] = self.exec_get_fixed_field(obj, slot)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::SetFixedField => {
                let val_reg = hi(code[*ip]);
                *ip += 1;
                let slot = code[*ip] as usize;
                *ip += 1;
                let obj = self.stack[base + first_reg];
                let val = self.stack[base + val_reg];
                self.exec_set_fixed_field(obj, slot, val)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::GetSuper => {
                let name_idx = code[*ip] as usize;
                *ip += 1;

                let this_val = self.stack[base];
                self.frames[frame_idx].ip = *ip;
                let val = self.exec_get_super_reg(this_val, name_idx, frame_idx, closure)?;
                let frame_idx2 = self.frames.len() - 1;
                self.stack[base + first_reg] = val;
                *ip = self.frames[frame_idx2].ip;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::GetSymbol => {
                let obj_reg = hi(code[*ip]);
                *ip += 1;
                let sym_idx = code[*ip] as usize;
                *ip += 1;
                let obj = self.stack[base + obj_reg];
                self.stack[base + first_reg] = self.exec_get_symbol(obj, sym_idx, closure)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::AssertNotNull => {
                let w1 = code[*ip];
                *ip += 1;
                let src = hi(w1);
                let v = self.stack[base + src];
                self.exec_assert_not_null(v)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::DeclareField => {
                let w1 = code[*ip];
                *ip += 1;
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let obj_reg = hi(w1);
                let obj = self.stack[base + obj_reg];
                self.exec_declare_field(obj, name_idx, frame_idx, closure)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::GetIndex => {
                let w1 = code[*ip];
                *ip += 1;
                let obj_reg = hi(w1);
                let idx_reg = lo(w1);
                let obj = self.stack[base + obj_reg];
                let key_nv = self.stack[base + idx_reg];
                let result = self.exec_get_index_nv(obj, key_nv)?;
                self.stack[base + first_reg] = result;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::ArrayGetIndex => {
                let w1 = code[*ip];
                *ip += 1;
                let obj_reg = hi(w1);
                let idx_reg = lo(w1);
                let obj = self.stack[base + obj_reg];
                let key_nv = self.stack[base + idx_reg];
                let result = self.exec_array_get_index(obj, key_nv)?;
                self.stack[base + first_reg] = result;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::SetIndex => {
                let w1 = code[*ip];
                *ip += 1;
                let idx_reg = hi(w1);
                let val_reg = lo(w1);
                let obj = self.stack[base + first_reg];
                let idx = self.stack[base + idx_reg];
                let val = self.stack[base + val_reg];
                self.exec_set_index(obj, idx, val)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::ArraySetIndex => {
                let w1 = code[*ip];
                *ip += 1;
                let idx_reg = hi(w1);
                let val_reg = lo(w1);
                let obj = self.stack[base + first_reg];
                let idx = self.stack[base + idx_reg];
                let val = self.stack[base + val_reg];
                self.exec_array_set_index(obj, idx, val)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::BuildArray | OpCode::BuildTuple => {
                let is_tuple = op == OpCode::BuildTuple;
                let w1 = code[*ip];
                *ip += 1;
                let w2 = code[*ip];
                *ip += 1;
                let (dest, start_reg) = (hi(w1), lo(w1));
                let count = hi(w2);
                let mut elems = Vec::with_capacity(count);
                for i in 0..count {
                    let nv = self.stack[base + start_reg + i];
                    elems.push(nv);
                }
                self.stack[base + dest] = if is_tuple {
                    self.heap.alloc_tuple_vm(elems)
                } else {
                    self.heap.alloc_array_vm(elems)
                };
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::BuildObject => {
                let w1 = code[*ip];
                *ip += 1;
                let (dest, count) = (hi(w1), lo(w1));
                let obj_nv = self.heap.alloc_object();
                for _ in 0..count {
                    let k_idx = code[*ip] as usize;
                    *ip += 1;
                    let w = code[*ip];
                    *ip += 1;
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
                    let val = self.stack[base + val_reg];
                    crate::exec::props::set_property(obj_nv, &key, val, &mut self.heap)?;
                }
                self.stack[base + dest] = obj_nv;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::BuildObjectWithShape | OpCode::BuildRecord => {
                let is_record = op == OpCode::BuildRecord;
                let w1 = code[*ip];
                *ip += 1;
                let shape_idx = code[*ip] as usize;
                *ip += 1;
                let (dest, start_reg) = (hi(w1), lo(w1));
                let shape = closure.proto.resolved_shape(shape_idx).expect("invalid shape");
                let count = shape.property_names.len();
                let mut values = Vec::with_capacity(count);
                for i in 0..count {
                    values.push(self.stack[base + start_reg + i]);
                }
                self.stack[base + dest] = if is_record {
                    self.heap.alloc_record_with_shape(&shape, values)
                } else {
                    self.heap.alloc_object_with_shape(&shape, values)
                };
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::ObjectRest => {
                let w1 = code[*ip];
                *ip += 1;
                let w2 = code[*ip];
                *ip += 1;
                let (dest, src) = (hi(w1), lo(w1));
                let skip_count = hi(w2);
                let mut skip_keys = Vec::with_capacity(skip_count);
                for _ in 0..skip_count {
                    let k_idx = code[*ip] as usize;
                    *ip += 1;
                    let key_nv = closure.constants[k_idx];
                    skip_keys.push(self.heap.str_val(key_nv).unwrap_or_else(|| {
                        closure.proto.chunk.constants[k_idx]
                            .as_str()
                            .unwrap_or("")
                            .into()
                    }));
                }
                let obj = self.stack[base + src];
                self.stack[base + dest] = self.exec_object_rest(obj, &skip_keys)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::ObjectKeys => {
                let src = hi(code[*ip]);
                *ip += 1;
                let obj = self.stack[base + src];
                self.stack[base + first_reg] = self.exec_object_keys(obj)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::ObjectMerge => {
                let src = hi(code[*ip]);
                *ip += 1;
                let dest_nv = self.stack[base + first_reg];
                let src_nv = self.stack[base + src];
                self.stack[base + first_reg] =
                    crate::exec::collections::object_merge(dest_nv, src_nv, &mut self.heap)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::WrapSpread => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.heap.extract(self.stack[base + src]);
                self.stack[base + first_reg] =
                    self.heap.intern(varn_types::Value::Spread(Box::new(v)));
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::ArrayLength => {
                let src = hi(code[*ip]);
                *ip += 1;
                let arr = self.stack[base + src];
                self.stack[base + first_reg] = self.exec_array_length(arr)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::ArrayPush => {
                let val_reg = hi(code[*ip]);
                *ip += 1;
                let arr = self.stack[base + first_reg];
                let val = self.stack[base + val_reg];
                self.exec_array_push(arr, val)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::ArrayPop => {
                let arr_reg = hi(code[*ip]);
                *ip += 1;
                let arr = self.stack[base + arr_reg];
                self.stack[base + first_reg] = self.exec_array_pop(arr)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::ArrayExtend => {
                let src_reg = hi(code[*ip]);
                *ip += 1;
                let arr = self.stack[base + first_reg];
                let src = self.stack[base + src_reg];
                self.exec_array_extend(arr, src)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::In => {
                let w1 = code[*ip];
                *ip += 1;
                let (src1, src2) = (hi(w1), lo(w1));
                let a = self.stack[base + src1];
                let b = self.stack[base + src2];
                let r = crate::exec::advanced::op_in(a, b, &self.heap);
                self.stack[base + first_reg] = VmValue::from_bool(r);
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::Instanceof => {
                let w1 = code[*ip];
                *ip += 1;
                let (src1, src2) = (hi(w1), lo(w1));
                let a = self.stack[base + src1];
                let b = self.stack[base + src2];
                let r = crate::exec::advanced::instanceof(a, b, &self.heap);
                self.stack[base + first_reg] = VmValue::from_bool(r);
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::Typeof => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack[base + src];
                let s = self.exec_typeof(v);
                self.stack[base + first_reg] = self.heap.alloc_str(s);
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::IsNull => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack[base + src];
                let res = VmValue::from_bool(v.is_null());
                self.stack[base + first_reg] = res;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::IsArray => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack[base + src];
                let is_arr = if v.is_heap() {
                    matches!(
                        self.heap.get(v.as_heap_idx()),
                        Some(crate::heap::HeapObj::Array(_))
                    )
                } else {
                    false
                };
                self.stack[base + first_reg] = VmValue::from_bool(is_arr);
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            _ => Ok(None),
        }
    }
}
