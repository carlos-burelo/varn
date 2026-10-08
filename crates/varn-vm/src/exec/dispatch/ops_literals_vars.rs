use crate::closure::VmClosure;
use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use varn_core::OpCode;

use super::{hi, lo};

impl ExecCtx {
    #[inline(always)]
    pub(super) fn exec_literals_vars_op(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        frame_idx: usize,
        closure: &VmClosure,
        first_reg: usize,
    ) -> VmResult<bool> {
        match op {
            OpCode::LoadNull => {
                self.stack
                    .unbox_into_reg(base, first_reg, crate::value::VmValue::null())?;
            }
            OpCode::LoadTrue => {
                self.stack
                    .unbox_into_reg(base, first_reg, crate::value::VmValue::bool_true())?;
            }
            OpCode::LoadFalse => {
                self.stack
                    .unbox_into_reg(base, first_reg, crate::value::VmValue::bool_false())?;
            }
            OpCode::LoadInt => {
                let val = code[*ip] as i16;
                *ip += 1;
                self.stack.unbox_into_reg(
                    base,
                    first_reg,
                    crate::value::VmValue::from_int(val as i64),
                )?;
            }
            OpCode::LoadIntZero => {
                self.stack
                    .unbox_into_reg(base, first_reg, crate::value::VmValue::from_int(0))?;
            }
            OpCode::LoadIntOne => {
                self.stack
                    .unbox_into_reg(base, first_reg, crate::value::VmValue::from_int(1))?;
            }
            OpCode::LoadIntMinusOne => {
                self.stack
                    .unbox_into_reg(base, first_reg, crate::value::VmValue::from_int(-1))?;
            }
            OpCode::LoadConst => {
                let cidx = code[*ip] as usize;
                *ip += 1;
                let nv = closure.constants[cidx];
                self.stack.unbox_into_reg(base, first_reg, nv)?;
            }
            OpCode::Move => {
                let w1 = code[*ip];
                *ip += 1;

                self.stack.mov(base, first_reg, hi(w1))?;
            }
            OpCode::LoadGlobalIdx => {
                let gidx = closure.module_base as usize + code[*ip] as usize;
                *ip += 1;
                debug_assert!(
                    gidx < self.globals_ref().values.len(),
                    "LoadGlobalIdx out of bounds: {gidx} >= {}",
                    self.globals_ref().values.len()
                );
                let nv = self.globals_ref().values[gidx];
                self.stack.unbox_into_reg(base, first_reg, nv)?;
                self.record_hotspot_global(gidx);
            }
            OpCode::LoadNativeGlobalIdx => {
                let gidx = code[*ip] as usize;
                *ip += 1;
                debug_assert!(
                    gidx < self.globals_ref().values.len(),
                    "LoadNativeGlobalIdx out of bounds: {gidx} >= {}",
                    self.globals_ref().values.len()
                );
                let nv = self.globals_ref().values[gidx];
                self.stack.unbox_into_reg(base, first_reg, nv)?;
                self.record_hotspot_global(gidx);
            }
            OpCode::StoreGlobalIdx | OpCode::DefineGlobalIdx => {
                let src = (code[*ip] >> 8) as usize;
                let gidx = closure.module_base as usize + code[*ip + 1] as usize;
                *ip += 2;
                debug_assert!(
                    gidx < self.globals_ref().values.len(),
                    "StoreGlobalIdx out of bounds: {gidx} >= {}",
                    self.globals_ref().values.len()
                );
                let val = self.stack.box_reg(base, src);
                self.globals_mut().set_by_index_unchecked(gidx, val);
            }
            OpCode::LoadGlobal | OpCode::StoreGlobal | OpCode::DefineGlobal => {
                self.frames[frame_idx].ip = *ip;
                self.exec_variable_op(op, code, ip, base, frame_idx, closure, first_reg)?;
            }
            OpCode::LoadUpvalue => {
                let w1 = code[*ip];
                *ip += 1;
                let (dest, uv) = (hi(w1), lo(w1));
                let nv = closure.upvalues[uv].read(&self.stack);
                self.stack.unbox_into_reg(base, dest, nv)?;
            }
            OpCode::StoreUpvalue => {
                let w1 = code[*ip];
                *ip += 1;
                let (uv, src) = (hi(w1), lo(w1));
                let val = self.stack.box_reg(base, src);
                closure.upvalues[uv].write(val, &mut self.stack)?;
            }
            OpCode::CloseUpvalue => {
                let w1 = code[*ip];
                *ip += 1;
                let lowest = hi(w1);
                self.close_upvalues_from_reg(base, lowest);
            }
            OpCode::Add
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
            | OpCode::GetIndex
            | OpCode::SetIndex
            | OpCode::ObjectRest
            | OpCode::ObjectKeys
            | OpCode::ObjectMerge
            | OpCode::GetProperty
            | OpCode::GetPropertyMaybe
            | OpCode::SetProperty
            | OpCode::GetFixedField
            | OpCode::SetFixedField
            | OpCode::GetSuper
            | OpCode::GetSymbol
            | OpCode::MakeClosure
            | OpCode::MakeClass
            | OpCode::Inherit
            | OpCode::Method
            | OpCode::DefineStatic
            | OpCode::DefineGetter
            | OpCode::DefineSetter
            | OpCode::DefineStaticGetter
            | OpCode::DefineStaticSetter
            | OpCode::DeclareLayout
            | OpCode::AllocInstance
            | OpCode::BindMethod
            | OpCode::Typeof
            | OpCode::Instanceof
            | OpCode::In
            | OpCode::IsNull
            | OpCode::IsArray
            | OpCode::AssertNotNull
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
            | OpCode::ArrayGetIndex
            | OpCode::ArraySetIndex
            | OpCode::CallNativeOp
            | OpCode::IntrinsicDirect
            | OpCode::BuildMap
            | OpCode::MapGetIndex
            | OpCode::MapSetIndex
            | OpCode::Convert
            | OpCode::BytesLength => return Ok(false),
        }

        Ok(true)
    }
}
