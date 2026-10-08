use crate::closure::VmClosure;
use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_core::OpCode;

mod call_spread;
mod calls;
mod class_ops;
mod enum_ops;
mod get_property;
mod method_calls;
mod misc_ops;
mod set_property;

impl ExecCtx {
    pub(super) fn exec_variable_op(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        frame_idx: usize,
        closure: &VmClosure,
        first_reg: usize,
    ) -> VmResult<()> {
        match op {
            OpCode::LoadGlobal => {
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let name_nv = closure.constants[name_idx];
                let name = self
                    .heap
                    .str_val(name_nv)
                    .ok_or_else(|| RuntimeError::new("LoadGlobal: non-string const"))?;
                let val = self
                    .globals_ref()
                    .get_by_name(&name)
                    .unwrap_or(VmValue::null());
                self.stack.unbox_into_reg(base, first_reg, val)?;
            }
            OpCode::StoreGlobal => {
                let src = (code[*ip] >> 8) as usize;
                *ip += 1;
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let name_nv = closure.constants[name_idx];
                let name = self
                    .heap
                    .str_val(name_nv)
                    .ok_or_else(|| RuntimeError::new("StoreGlobal: non-string const"))?;
                let val = self.stack.box_reg(base, src);
                self.globals_mut().set_by_name(&name, val);
            }
            OpCode::DefineGlobal => {
                let src = (code[*ip] >> 8) as usize;
                *ip += 1;
                let name_idx = code[*ip] as usize;
                *ip += 1;
                let name_nv = closure.constants[name_idx];
                let name = self
                    .heap
                    .str_val(name_nv)
                    .ok_or_else(|| RuntimeError::new("DefineGlobal: non-string const"))?;
                let val = self.stack.box_reg(base, src);
                self.globals_mut().define(&name, val);
            }
            OpCode::LoadGlobalIdx => {
                let idx = code[*ip] as usize;
                *ip += 1;
                let val = self
                    .globals_ref()
                    .get_by_index(idx)
                    .unwrap_or(VmValue::null());
                self.stack.unbox_into_reg(base, first_reg, val)?;
                self.record_hotspot_global(idx);
            }
            OpCode::StoreGlobalIdx => {
                let src = (code[*ip] >> 8) as usize;
                *ip += 1;
                let idx = code[*ip] as usize;
                *ip += 1;
                let val = self.stack.box_reg(base, src);
                self.globals_mut().set_by_index(idx, val);
            }
            OpCode::DefineGlobalIdx => {
                let src = (code[*ip] >> 8) as usize;
                *ip += 1;
                let idx = code[*ip] as usize;
                *ip += 1;
                let val = self.stack.box_reg(base, src);
                self.globals_mut().set_by_index(idx, val);
            }
            OpCode::LoadConst
            | OpCode::LoadNull
            | OpCode::LoadTrue
            | OpCode::LoadFalse
            | OpCode::LoadInt
            | OpCode::Move
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
            | OpCode::ArrayGetIndex
            | OpCode::ArraySetIndex
            | OpCode::CallNativeOp
            | OpCode::IntrinsicDirect
            | OpCode::LoadNativeGlobalIdx
            | OpCode::BuildMap
            | OpCode::MapGetIndex
            | OpCode::MapSetIndex
            | OpCode::Convert
            | OpCode::BytesLength => {}
        }
        self.frames[frame_idx].ip = *ip;
        Ok(())
    }
}
