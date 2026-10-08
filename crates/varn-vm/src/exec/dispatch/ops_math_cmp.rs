mod arith_float;
mod arith_generic;
mod arith_int;
mod cmp_float;
mod cmp_generic_str;
mod cmp_int;

use crate::closure::VmClosure;
use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use varn_core::OpCode;

#[cold]
#[inline(never)]
fn int_overflow(op: &str, a: i64, b: i64) -> crate::error::RuntimeError {
    crate::error::RuntimeError::integer_overflow(format!(
        "integer overflow: {a} {op} {b} is outside int ({}..={})",
        varn_core::INT_MIN,
        varn_core::INT_MAX
    ))
}

impl ExecCtx {
    #[inline(always)]
    pub(super) fn exec_math_cmp_op(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        _frame_idx: usize,
        _closure: &VmClosure,
        first_reg: usize,
    ) -> VmResult<bool> {
        match op {
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
                self.exec_generic_binary(op, code, ip, base, first_reg)?;
            }

            OpCode::Negate | OpCode::Not => {
                self.exec_unary(op, code, ip, base, first_reg)?;
            }

            OpCode::AddImm | OpCode::SubImm => {
                self.exec_imm(op, code, ip, base, first_reg)?;
            }

            OpCode::AddInt
            | OpCode::SubInt
            | OpCode::MulInt
            | OpCode::DivInt
            | OpCode::ModInt
            | OpCode::PowInt => {
                self.exec_int_arith(op, code, ip, base, first_reg)?;
            }

            OpCode::LtInt
            | OpCode::GtInt
            | OpCode::LteInt
            | OpCode::GteInt
            | OpCode::EqInt
            | OpCode::NeqInt => {
                self.exec_int_cmp(op, code, ip, base, first_reg)?;
            }

            OpCode::AddFloat
            | OpCode::SubFloat
            | OpCode::MulFloat
            | OpCode::DivFloat
            | OpCode::ModFloat
            | OpCode::PowFloat => {
                self.exec_float_arith(op, code, ip, base, first_reg)?;
            }

            OpCode::LtFloat
            | OpCode::GtFloat
            | OpCode::LteFloat
            | OpCode::GteFloat
            | OpCode::EqFloat
            | OpCode::NeqFloat => {
                self.exec_float_cmp(op, code, ip, base, first_reg)?;
            }

            OpCode::Eq | OpCode::Neq | OpCode::Lt | OpCode::Lte | OpCode::Gt | OpCode::Gte => {
                self.exec_generic_cmp(op, code, ip, base, first_reg)?;
            }
            OpCode::ToString
            | OpCode::StrConcat
            | OpCode::BuildStr
            | OpCode::StrLength
            | OpCode::StrSlice => {
                self.exec_str_op(op, code, ip, base, first_reg)?;
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
            | OpCode::LoadIntZero
            | OpCode::LoadIntOne
            | OpCode::LoadIntMinusOne
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
            | OpCode::BytesLength => return Ok(false),
        }

        Ok(true)
    }
}
