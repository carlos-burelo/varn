use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use varn_core::OpCode;

use super::super::{hi, lo};

impl ExecCtx {
    pub(super) fn exec_generic_binary(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        first_reg: usize,
    ) -> VmResult<()> {
        let w1 = code[*ip];
        *ip += 1;
        let a = self.stack.box_reg(base, hi(w1));
        let b = self.stack.box_reg(base, lo(w1));
        let r = match op {
            OpCode::Add => crate::exec::arith::add(a, b, &mut self.heap)?,
            OpCode::Sub => crate::exec::arith::sub(a, b, &mut self.heap)?,
            OpCode::Mul => crate::exec::arith::mul(a, b, &mut self.heap)?,
            OpCode::Div => crate::exec::arith::div(a, b, &mut self.heap)?,
            OpCode::Mod => crate::exec::arith::modulo(a, b, &mut self.heap)?,
            OpCode::Pow => crate::exec::arith::pow(a, b, &mut self.heap)?,
            OpCode::BitAnd => crate::exec::arith::bit_and(a, b, &mut self.heap),
            OpCode::BitOr => crate::exec::arith::bit_or(a, b, &mut self.heap),
            OpCode::BitXor => crate::exec::arith::bit_xor(a, b, &mut self.heap),
            OpCode::Shl => crate::exec::arith::shl(a, b, &mut self.heap),
            OpCode::Shr => crate::exec::arith::shr(a, b, &mut self.heap),
            OpCode::Ushr => crate::exec::arith::ushr(a, b, &mut self.heap),
            OpCode::LoadConst | OpCode::LoadNull | OpCode::LoadTrue | OpCode::LoadFalse | OpCode::LoadInt | OpCode::Move | OpCode::LoadGlobal | OpCode::StoreGlobal | OpCode::DefineGlobal | OpCode::DefineGlobalIdx | OpCode::LoadGlobalIdx | OpCode::StoreGlobalIdx | OpCode::LoadUpvalue | OpCode::StoreUpvalue | OpCode::CloseUpvalue | OpCode::Negate | OpCode::Not | OpCode::ToString | OpCode::Eq | OpCode::Neq | OpCode::Lt | OpCode::Lte | OpCode::Gt | OpCode::Gte | OpCode::Jump | OpCode::JumpIfFalse | OpCode::JumpIfTrue | OpCode::Loop | OpCode::Call | OpCode::CallMethod | OpCode::InvokeVirtual | OpCode::CallSpread | OpCode::Return | OpCode::BuildArray | OpCode::BuildTuple | OpCode::BuildObject | OpCode::BuildObjectWithShape | OpCode::BuildRecord | OpCode::GetIndex | OpCode::SetIndex | OpCode::ObjectRest | OpCode::ObjectKeys | OpCode::ObjectMerge | OpCode::GetProperty | OpCode::GetPropertyMaybe | OpCode::SetProperty | OpCode::GetFixedField | OpCode::SetFixedField | OpCode::GetSuper | OpCode::GetSymbol | OpCode::MakeClosure | OpCode::MakeClass | OpCode::Inherit | OpCode::Method | OpCode::DefineStatic | OpCode::DefineGetter | OpCode::DefineSetter | OpCode::DefineStaticGetter | OpCode::DefineStaticSetter | OpCode::DeclareLayout | OpCode::AllocInstance | OpCode::BindMethod | OpCode::Typeof | OpCode::Instanceof | OpCode::In | OpCode::IsNull | OpCode::IsArray | OpCode::AssertNotNull | OpCode::StrConcat | OpCode::StrLength | OpCode::StrSlice | OpCode::ArrayLength | OpCode::ArrayPush | OpCode::ArrayPop | OpCode::ArrayExtend | OpCode::WrapSpread | OpCode::MakeEnumVariant | OpCode::GetEnumTag | OpCode::Await | OpCode::Spawn | OpCode::Yield | OpCode::Try | OpCode::Throw | OpCode::PopTry | OpCode::LoadModule | OpCode::LoadModuleSlot | OpCode::StoreModuleSlot | OpCode::InvokeRuntimeStatic | OpCode::AddImm | OpCode::SubImm | OpCode::BuildStr | OpCode::LoadIntZero | OpCode::LoadIntOne | OpCode::LoadIntMinusOne | OpCode::AddInt | OpCode::SubInt | OpCode::MulInt | OpCode::DivInt | OpCode::LtInt | OpCode::GtInt | OpCode::LteInt | OpCode::GteInt | OpCode::EqInt | OpCode::NeqInt | OpCode::AddFloat | OpCode::SubFloat | OpCode::MulFloat | OpCode::DivFloat | OpCode::ModFloat | OpCode::PowFloat | OpCode::LtFloat | OpCode::GtFloat | OpCode::LteFloat | OpCode::GteFloat | OpCode::EqFloat | OpCode::NeqFloat | OpCode::ModInt | OpCode::PowInt | OpCode::Intrinsic | OpCode::LoadStaticFn | OpCode::CallSelf | OpCode::Nop | OpCode::ArrayGetIndex | OpCode::ArraySetIndex | OpCode::CallNativeOp | OpCode::IntrinsicDirect | OpCode::LoadNativeGlobalIdx | OpCode::BuildMap | OpCode::MapGetIndex | OpCode::MapSetIndex | OpCode::Convert | OpCode::BytesLength => unreachable!(),
        };
        self.stack.unbox_into_reg(base, first_reg, r)?;
        Ok(())
    }

    pub(super) fn exec_unary(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        first_reg: usize,
    ) -> VmResult<()> {
        let src = hi(code[*ip]);
        *ip += 1;
        let v = self.stack.box_reg(base, src);
        let r = match op {
            OpCode::Negate => crate::exec::arith::negate(v, &mut self.heap)?,
            OpCode::Not => crate::exec::compare::logical_not(v),
            OpCode::LoadConst | OpCode::LoadNull | OpCode::LoadTrue | OpCode::LoadFalse | OpCode::LoadInt | OpCode::Move | OpCode::LoadGlobal | OpCode::StoreGlobal | OpCode::DefineGlobal | OpCode::DefineGlobalIdx | OpCode::LoadGlobalIdx | OpCode::StoreGlobalIdx | OpCode::LoadUpvalue | OpCode::StoreUpvalue | OpCode::CloseUpvalue | OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod | OpCode::Pow | OpCode::ToString | OpCode::Eq | OpCode::Neq | OpCode::Lt | OpCode::Lte | OpCode::Gt | OpCode::Gte | OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::Shl | OpCode::Shr | OpCode::Ushr | OpCode::Jump | OpCode::JumpIfFalse | OpCode::JumpIfTrue | OpCode::Loop | OpCode::Call | OpCode::CallMethod | OpCode::InvokeVirtual | OpCode::CallSpread | OpCode::Return | OpCode::BuildArray | OpCode::BuildTuple | OpCode::BuildObject | OpCode::BuildObjectWithShape | OpCode::BuildRecord | OpCode::GetIndex | OpCode::SetIndex | OpCode::ObjectRest | OpCode::ObjectKeys | OpCode::ObjectMerge | OpCode::GetProperty | OpCode::GetPropertyMaybe | OpCode::SetProperty | OpCode::GetFixedField | OpCode::SetFixedField | OpCode::GetSuper | OpCode::GetSymbol | OpCode::MakeClosure | OpCode::MakeClass | OpCode::Inherit | OpCode::Method | OpCode::DefineStatic | OpCode::DefineGetter | OpCode::DefineSetter | OpCode::DefineStaticGetter | OpCode::DefineStaticSetter | OpCode::DeclareLayout | OpCode::AllocInstance | OpCode::BindMethod | OpCode::Typeof | OpCode::Instanceof | OpCode::In | OpCode::IsNull | OpCode::IsArray | OpCode::AssertNotNull | OpCode::StrConcat | OpCode::StrLength | OpCode::StrSlice | OpCode::ArrayLength | OpCode::ArrayPush | OpCode::ArrayPop | OpCode::ArrayExtend | OpCode::WrapSpread | OpCode::MakeEnumVariant | OpCode::GetEnumTag | OpCode::Await | OpCode::Spawn | OpCode::Yield | OpCode::Try | OpCode::Throw | OpCode::PopTry | OpCode::LoadModule | OpCode::LoadModuleSlot | OpCode::StoreModuleSlot | OpCode::InvokeRuntimeStatic | OpCode::AddImm | OpCode::SubImm | OpCode::BuildStr | OpCode::LoadIntZero | OpCode::LoadIntOne | OpCode::LoadIntMinusOne | OpCode::AddInt | OpCode::SubInt | OpCode::MulInt | OpCode::DivInt | OpCode::LtInt | OpCode::GtInt | OpCode::LteInt | OpCode::GteInt | OpCode::EqInt | OpCode::NeqInt | OpCode::AddFloat | OpCode::SubFloat | OpCode::MulFloat | OpCode::DivFloat | OpCode::ModFloat | OpCode::PowFloat | OpCode::LtFloat | OpCode::GtFloat | OpCode::LteFloat | OpCode::GteFloat | OpCode::EqFloat | OpCode::NeqFloat | OpCode::ModInt | OpCode::PowInt | OpCode::Intrinsic | OpCode::LoadStaticFn | OpCode::CallSelf | OpCode::Nop | OpCode::ArrayGetIndex | OpCode::ArraySetIndex | OpCode::CallNativeOp | OpCode::IntrinsicDirect | OpCode::LoadNativeGlobalIdx | OpCode::BuildMap | OpCode::MapGetIndex | OpCode::MapSetIndex | OpCode::Convert | OpCode::BytesLength => unreachable!(),
        };
        self.stack.unbox_into_reg(base, first_reg, r)?;
        Ok(())
    }
}
