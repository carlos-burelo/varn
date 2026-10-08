use crate::closure::VmClosure;
use crate::error::VmResult;
use crate::exec::closures::UpvalueSrc;
use crate::exec::ctx::ExecCtx;
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
                let proto_idx = code[*ip + 1] as usize;
                let (dest, uv_count) = (hi(w1), lo(w1));
                let descs = &code[*ip + 2..*ip + 2 + uv_count];
                *ip += 2 + uv_count;
                let upvalues = descs.iter().map(|&w| UpvalueSrc::from_bytecode(w));
                let val = self.make_closure(closure, proto_idx, base, upvalues)?;
                self.stack.unbox_into_reg(base, dest, val)?;
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
                let obj = self.stack.box_reg(base, obj_reg);
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
                let obj = self.stack.box_reg(base, obj_reg);
                let name_nv = closure.constants[name_idx];
                let name = self.heap.str_val(name_nv).unwrap_or_else(|| {
                    closure.proto.chunk.constants[name_idx]
                        .as_str()
                        .unwrap_or("")
                        .into()
                });
                let result = crate::exec::props::get_property_maybe(obj, &name, &mut self.heap);
                self.stack.unbox_into_reg(base, first_reg, result)?;
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
                let obj = self.stack.box_reg(base, obj_reg);
                let val = self.stack.box_reg(base, val_reg);
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
                let tag = (code[*ip] & 0xFF) as u8;
                *ip += 1;
                let slot = code[*ip] as usize;
                *ip += 1;
                let offset = code[*ip];
                *ip += 1;
                let obj = self.stack.box_reg(base, obj_reg);
                let r = self.exec_get_fixed_field(obj, slot, offset, tag)?;
                self.stack.unbox_into_reg(base, first_reg, r)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::SetFixedField => {
                let val_reg = hi(code[*ip]);
                let tag = (code[*ip] & 0xFF) as u8;
                *ip += 1;
                let slot = code[*ip] as usize;
                *ip += 1;
                let offset = code[*ip];
                *ip += 1;
                let obj = self.stack.box_reg(base, first_reg);
                let val = self.stack.box_reg(base, val_reg);
                self.exec_set_fixed_field(obj, slot, offset, tag, val)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::GetSuper => {
                let name_idx = code[*ip] as usize;
                *ip += 1;

                let this_val = self.stack.box_reg(base, 0);
                self.frames[frame_idx].ip = *ip;
                let val = self.exec_get_super_reg(this_val, name_idx, frame_idx, closure)?;
                let frame_idx2 = self.frames.len() - 1;
                self.stack.unbox_into_reg(base, first_reg, val)?;
                *ip = self.frames[frame_idx2].ip;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::GetSymbol => {
                let obj_reg = hi(code[*ip]);
                *ip += 1;
                let sym_idx = code[*ip] as usize;
                *ip += 1;
                let obj = self.stack.box_reg(base, obj_reg);
                let r = self.exec_get_symbol(obj, sym_idx, closure)?;
                self.stack.unbox_into_reg(base, first_reg, r)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::AssertNotNull => {
                let w1 = code[*ip];
                *ip += 1;
                let src = hi(w1);
                let v = self.stack.box_reg(base, src);
                self.exec_assert_not_null(v)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::AllocInstance => {
                let class = self.stack.box_reg(base, hi(code[*ip]));
                *ip += 1;
                let instance = crate::exec::class::op_alloc_instance(class, &mut self.heap)?;
                self.stack.unbox_into_reg(base, first_reg, instance)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::DeclareLayout => {
                let w1 = code[*ip];
                *ip += 1;
                let layout_idx = code[*ip] as usize;
                *ip += 1;
                let class = self.stack.box_reg(base, hi(w1));
                let layout = crate::exec::class::pool_layout(&closure.proto, layout_idx)?;
                crate::exec::class::op_declare_layout(class, layout, &mut self.heap)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::GetIndex => {
                let w1 = code[*ip];
                *ip += 1;
                let obj_reg = hi(w1);
                let idx_reg = lo(w1);
                let obj = self.stack.box_reg(base, obj_reg);
                let key_nv = self.stack.box_reg(base, idx_reg);
                let result = self.exec_get_index(obj, key_nv)?;
                self.stack.unbox_into_reg(base, first_reg, result)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::ArrayGetIndex => {
                let w1 = code[*ip];
                *ip += 1;
                let obj_reg = hi(w1);
                let idx_reg = lo(w1);
                let obj = self.stack.box_reg(base, obj_reg);
                let key_nv = self.stack.box_reg(base, idx_reg);
                let result = self.exec_array_get_index(obj, key_nv)?;
                self.stack.unbox_into_reg(base, first_reg, result)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::MapGetIndex => {
                let w1 = code[*ip];
                *ip += 1;
                let obj_reg = hi(w1);
                let idx_reg = lo(w1);
                let obj = self.stack.box_reg(base, obj_reg);
                let key_nv = self.stack.box_reg(base, idx_reg);
                let result = self.exec_map_get_index(obj, key_nv)?;
                self.stack.unbox_into_reg(base, first_reg, result)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::SetIndex => {
                let w1 = code[*ip];
                *ip += 1;
                let idx_reg = hi(w1);
                let val_reg = lo(w1);
                let obj = self.stack.box_reg(base, first_reg);
                let idx = self.stack.box_reg(base, idx_reg);
                let val = self.stack.box_reg(base, val_reg);
                self.exec_set_index(obj, idx, val)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::ArraySetIndex => {
                let w1 = code[*ip];
                *ip += 1;
                let idx_reg = hi(w1);
                let val_reg = lo(w1);
                let obj = self.stack.box_reg(base, first_reg);
                let idx = self.stack.box_reg(base, idx_reg);
                let val = self.stack.box_reg(base, val_reg);
                self.exec_array_set_index(obj, idx, val)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::MapSetIndex => {
                let w1 = code[*ip];
                *ip += 1;
                let idx_reg = hi(w1);
                let val_reg = lo(w1);
                let obj = self.stack.box_reg(base, first_reg);
                let idx = self.stack.box_reg(base, idx_reg);
                let val = self.stack.box_reg(base, val_reg);
                self.exec_map_set_index(obj, idx, val)?;
                Ok(Some(ObjectFlow::ContinueInstruction))
            }
            OpCode::LoadConst
            | OpCode::LoadNull
            | OpCode::LoadTrue
            | OpCode::LoadFalse
            | OpCode::LoadInt
            | OpCode::Move
            | OpCode::LoadGlobal
            | OpCode::StoreGlobal
            | OpCode::DefineGlobal
            | OpCode::DefineGlobalIdx
            | OpCode::LoadGlobalIdx
            | OpCode::StoreGlobalIdx
            | OpCode::LoadUpvalue
            | OpCode::StoreUpvalue
            | OpCode::CloseUpvalue
            | OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Mod
            | OpCode::Pow
            | OpCode::Negate
            | OpCode::Not
            | OpCode::ToString
            | OpCode::Eq
            | OpCode::Neq
            | OpCode::Lt
            | OpCode::Lte
            | OpCode::Gt
            | OpCode::Gte
            | OpCode::BitAnd
            | OpCode::BitOr
            | OpCode::BitXor
            | OpCode::Shl
            | OpCode::Shr
            | OpCode::Ushr
            | OpCode::Jump
            | OpCode::JumpIfFalse
            | OpCode::JumpIfTrue
            | OpCode::Loop
            | OpCode::Call
            | OpCode::CallMethod
            | OpCode::InvokeVirtual
            | OpCode::CallSpread
            | OpCode::Return
            | OpCode::BuildArray
            | OpCode::BuildTuple
            | OpCode::BuildObject
            | OpCode::BuildObjectWithShape
            | OpCode::BuildRecord
            | OpCode::ObjectRest
            | OpCode::ObjectKeys
            | OpCode::ObjectMerge
            | OpCode::MakeClass
            | OpCode::Inherit
            | OpCode::Method
            | OpCode::DefineStatic
            | OpCode::DefineGetter
            | OpCode::DefineSetter
            | OpCode::DefineStaticGetter
            | OpCode::DefineStaticSetter
            | OpCode::BindMethod
            | OpCode::Typeof
            | OpCode::Instanceof
            | OpCode::In
            | OpCode::IsNull
            | OpCode::IsArray
            | OpCode::StrConcat
            | OpCode::StrLength
            | OpCode::StrSlice
            | OpCode::ArrayLength
            | OpCode::ArrayPush
            | OpCode::ArrayPop
            | OpCode::ArrayExtend
            | OpCode::WrapSpread
            | OpCode::MakeEnumVariant
            | OpCode::GetEnumTag
            | OpCode::Await
            | OpCode::Spawn
            | OpCode::Yield
            | OpCode::Try
            | OpCode::Throw
            | OpCode::PopTry
            | OpCode::LoadModule
            | OpCode::LoadModuleSlot
            | OpCode::StoreModuleSlot
            | OpCode::InvokeRuntimeStatic
            | OpCode::AddImm
            | OpCode::SubImm
            | OpCode::BuildStr
            | OpCode::LoadIntZero
            | OpCode::LoadIntOne
            | OpCode::LoadIntMinusOne
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
            | OpCode::ModFloat
            | OpCode::PowFloat
            | OpCode::LtFloat
            | OpCode::GtFloat
            | OpCode::LteFloat
            | OpCode::GteFloat
            | OpCode::EqFloat
            | OpCode::NeqFloat
            | OpCode::ModInt
            | OpCode::PowInt
            | OpCode::Intrinsic
            | OpCode::LoadStaticFn
            | OpCode::CallSelf
            | OpCode::Nop
            | OpCode::CallNativeOp
            | OpCode::IntrinsicDirect
            | OpCode::LoadNativeGlobalIdx
            | OpCode::BuildMap
            | OpCode::Convert
            | OpCode::BytesLength => {
                self.exec_build_op(op, code, ip, base, frame_idx, closure, first_reg)
            }
        }
    }
}
