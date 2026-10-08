use super::hi;
use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use varn_core::OpCode;

impl ExecCtx {
    pub(super) fn exec_misc_op(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        first_reg: usize,
        closure: &crate::closure::VmClosure,
    ) -> VmResult<()> {
        match op {
            OpCode::Try => {
                let w1 = code[*ip];
                *ip += 1;
                let err_reg = hi(w1) as u8;
                let offset_hi = code[*ip] as u32;
                let offset_lo = code[*ip + 1] as u32;
                let catch_offset = ((offset_hi << 16) | offset_lo) as usize;
                *ip += 2;
                let catch_ip = *ip + catch_offset;
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
            OpCode::GetEnumTag => {
                let src = hi(code[*ip]);
                *ip += 1;
                let v = self.stack.box_reg(base, src);
                let tag = (self.exec_get_enum_tag(v))?;
                (self.stack.unbox_into_reg(base, first_reg, tag))?;
            }
            OpCode::Spawn => {
                let w1 = code[*ip];
                *ip += 1;
                let (dest, src) = (first_reg, hi(w1));

                let task_val = self.stack.box_reg(base, src);
                let spawned = (self.exec_spawn(task_val))?;
                (self.stack.unbox_into_reg(base, dest, spawned))?;
            }
            OpCode::LoadStaticFn => {
                let proto_idx = code[*ip] as usize;
                *ip += 1;
                let val = (self.make_closure(closure, proto_idx, base, std::iter::empty()))?;
                (self.stack.unbox_into_reg(base, first_reg, val))?;
            }

            OpCode::LoadConst | OpCode::LoadNull | OpCode::LoadTrue | OpCode::LoadFalse | OpCode::LoadInt | OpCode::Move | OpCode::LoadGlobal | OpCode::StoreGlobal | OpCode::DefineGlobal | OpCode::DefineGlobalIdx | OpCode::LoadGlobalIdx | OpCode::StoreGlobalIdx | OpCode::LoadUpvalue | OpCode::StoreUpvalue | OpCode::CloseUpvalue | OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod | OpCode::Pow | OpCode::Negate | OpCode::Not | OpCode::ToString | OpCode::Eq | OpCode::Neq | OpCode::Lt | OpCode::Lte | OpCode::Gt | OpCode::Gte | OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::Shl | OpCode::Shr | OpCode::Ushr | OpCode::Jump | OpCode::JumpIfFalse | OpCode::JumpIfTrue | OpCode::Loop | OpCode::Call | OpCode::CallMethod | OpCode::InvokeVirtual | OpCode::CallSpread | OpCode::Return | OpCode::BuildArray | OpCode::BuildTuple | OpCode::BuildObject | OpCode::BuildObjectWithShape | OpCode::BuildRecord | OpCode::GetIndex | OpCode::SetIndex | OpCode::ObjectRest | OpCode::ObjectKeys | OpCode::ObjectMerge | OpCode::GetProperty | OpCode::GetPropertyMaybe | OpCode::SetProperty | OpCode::GetFixedField | OpCode::SetFixedField | OpCode::GetSuper | OpCode::GetSymbol | OpCode::MakeClosure | OpCode::MakeClass | OpCode::Inherit | OpCode::Method | OpCode::DefineStatic | OpCode::DefineGetter | OpCode::DefineSetter | OpCode::DefineStaticGetter | OpCode::DefineStaticSetter | OpCode::DeclareLayout | OpCode::AllocInstance | OpCode::BindMethod | OpCode::Typeof | OpCode::Instanceof | OpCode::In | OpCode::IsNull | OpCode::IsArray | OpCode::AssertNotNull | OpCode::StrConcat | OpCode::StrLength | OpCode::StrSlice | OpCode::ArrayLength | OpCode::ArrayPush | OpCode::ArrayPop | OpCode::ArrayExtend | OpCode::WrapSpread | OpCode::MakeEnumVariant | OpCode::Await | OpCode::Yield | OpCode::Throw | OpCode::LoadModule | OpCode::LoadModuleSlot | OpCode::StoreModuleSlot | OpCode::InvokeRuntimeStatic | OpCode::AddImm | OpCode::SubImm | OpCode::BuildStr | OpCode::LoadIntZero | OpCode::LoadIntOne | OpCode::LoadIntMinusOne | OpCode::AddInt | OpCode::SubInt | OpCode::MulInt | OpCode::DivInt | OpCode::LtInt | OpCode::GtInt | OpCode::LteInt | OpCode::GteInt | OpCode::EqInt | OpCode::NeqInt | OpCode::AddFloat | OpCode::SubFloat | OpCode::MulFloat | OpCode::DivFloat | OpCode::ModFloat | OpCode::PowFloat | OpCode::LtFloat | OpCode::GtFloat | OpCode::LteFloat | OpCode::GteFloat | OpCode::EqFloat | OpCode::NeqFloat | OpCode::ModInt | OpCode::PowInt | OpCode::Intrinsic | OpCode::CallSelf | OpCode::Nop | OpCode::ArrayGetIndex | OpCode::ArraySetIndex | OpCode::CallNativeOp | OpCode::IntrinsicDirect | OpCode::LoadNativeGlobalIdx | OpCode::BuildMap | OpCode::MapGetIndex | OpCode::MapSetIndex | OpCode::Convert | OpCode::BytesLength => unreachable!("exec_misc_op called with a non-misc opcode"),
        }
        Ok(())
    }
}
