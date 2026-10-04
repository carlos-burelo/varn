use super::ops_objects_collections::ObjectFlow;
use super::{hi, lo};
use crate::closure::VmClosure;
use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_core::OpCode;

impl ExecCtx {
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn exec_build_op(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        frame_idx: usize,
        closure: &VmClosure,
        first_reg: usize,
    ) -> VmResult<Option<ObjectFlow>> {
        let _ = (frame_idx, closure);
        match op {
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
                    let nv = self.stack.box_reg(base, start_reg + i);
                    elems.push(nv);
                }
                let built = if is_tuple {
                    self.heap.alloc_tuple_vm(elems)
                } else {
                    self.heap.alloc_array_vm(elems)
                };
                self.stack.unbox_into_reg(base, dest, built)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::BuildMap => {
                let w1 = code[*ip];
                *ip += 1;
                let w2 = code[*ip];
                *ip += 1;
                let (dest, start_reg) = (hi(w1), lo(w1));
                let count = hi(w2);
                if count == 0 {
                    self.stack
                        .unbox_into_reg(base, dest, self.heap.alloc_empty_map_vm())?;
                    return Ok(Some(ObjectFlow::ContinueInstruction));
                }
                let mut map = varn_types::value::ValueMap::with_capacity(count);
                for i in 0..count {
                    let k_nv = self.stack.box_reg(base, start_reg + i * 2);
                    let v_nv = self.stack.box_reg(base, start_reg + i * 2 + 1);
                    let key = self.heap.canonical_map_key(k_nv);
                    map.insert(key, v_nv);
                }
                self.stack
                    .unbox_into_reg(base, dest, self.heap.alloc_map_vm(map))?;
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
                    let val = self.stack.box_reg(base, val_reg);
                    crate::exec::props::set_property(obj_nv, &key, val, &mut self.heap)?;
                }
                self.stack.unbox_into_reg(base, dest, obj_nv)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::BuildObjectWithShape | OpCode::BuildRecord => {
                let is_record = op == OpCode::BuildRecord;
                let w1 = code[*ip];
                *ip += 1;
                let shape_idx = code[*ip] as usize;
                *ip += 1;
                let (dest, start_reg) = (hi(w1), lo(w1));
                let shape = closure
                    .proto
                    .resolved_shape(shape_idx)
                    .expect("invalid shape");
                let count = shape.property_names.len();
                let boxed: Vec<VmValue> = self.stack.box_range(base, start_reg, count);
                self.stack.unbox_into_reg(
                    base,
                    dest,
                    if is_record {
                        self.heap.alloc_record_with_shape_slice(&shape, &boxed)
                    } else {
                        self.heap.alloc_object_with_shape_slice(&shape, &boxed)
                    },
                )?;
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
                let obj = self.stack.box_reg(base, src);
                let r = self.exec_object_rest(obj, &skip_keys)?;
                self.stack.unbox_into_reg(base, dest, r)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::ObjectKeys => {
                let src = hi(code[*ip]);
                *ip += 1;
                let obj = self.stack.box_reg(base, src);
                let r = self.exec_object_keys(obj)?;
                self.stack.unbox_into_reg(base, first_reg, r)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::ObjectMerge => {
                let src = hi(code[*ip]);
                *ip += 1;
                let dest_nv = self.stack.box_reg(base, first_reg);
                let src_nv = self.stack.box_reg(base, src);
                self.stack.unbox_into_reg(
                    base,
                    first_reg,
                    crate::exec::collections::object_merge(dest_nv, src_nv, &mut self.heap)?,
                )?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::WrapSpread => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack.box_reg(base, src);
                let spread = self.heap.alloc_spread(v);
                self.stack.unbox_into_reg(base, first_reg, spread)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::ArrayLength => {
                let src = hi(code[*ip]);
                *ip += 1;
                let arr = self.stack.box_reg(base, src);
                let r = self.exec_array_length(arr)?;
                self.stack.unbox_into_reg(base, first_reg, r)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::BytesLength => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack.box_reg(base, src);
                let r = self.exec_bytes_length(v)?;
                self.stack.unbox_into_reg(base, first_reg, r)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::ArrayPush => {
                let val_reg = hi(code[*ip]);
                *ip += 1;
                let arr = self.stack.box_reg(base, first_reg);
                let val = self.stack.box_reg(base, val_reg);
                self.exec_array_push(arr, val)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::ArrayPop => {
                let arr_reg = hi(code[*ip]);
                *ip += 1;
                let arr = self.stack.box_reg(base, arr_reg);
                let r = self.exec_array_pop(arr)?;
                self.stack.unbox_into_reg(base, first_reg, r)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::ArrayExtend => {
                let src_reg = hi(code[*ip]);
                *ip += 1;
                let arr = self.stack.box_reg(base, first_reg);
                let src = self.stack.box_reg(base, src_reg);
                self.exec_array_extend(arr, src)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::In => {
                let w1 = code[*ip];
                *ip += 1;
                let (src1, src2) = (hi(w1), lo(w1));
                let a = self.stack.box_reg(base, src1);
                let b = self.stack.box_reg(base, src2);
                let r = crate::exec::advanced::op_in(a, b, &self.heap);
                self.stack
                    .unbox_into_reg(base, first_reg, VmValue::from_bool(r))?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::Instanceof => {
                let w1 = code[*ip];
                *ip += 1;
                let (src1, src2) = (hi(w1), lo(w1));
                let a = self.stack.box_reg(base, src1);
                let b = self.stack.box_reg(base, src2);
                let r = crate::exec::advanced::instanceof(a, b, &self.heap);
                self.stack
                    .unbox_into_reg(base, first_reg, VmValue::from_bool(r))?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::Typeof => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack.box_reg(base, src);
                let s = self.exec_typeof(v);
                self.stack
                    .unbox_into_reg(base, first_reg, self.heap.alloc_str(s))?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::IsNull => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack.box_reg(base, src);
                let res = VmValue::from_bool(v.is_null());
                self.stack.unbox_into_reg(base, first_reg, res)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::IsArray => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack.box_reg(base, src);
                let is_arr = if v.is_heap() {
                    matches!(
                        self.heap.get(v.as_heap_idx()),
                        Some(crate::heap::HeapObj::Array(_))
                    )
                } else {
                    false
                };
                self.stack
                    .unbox_into_reg(base, first_reg, VmValue::from_bool(is_arr))?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            _ => Ok(None),
        }
    }
}
