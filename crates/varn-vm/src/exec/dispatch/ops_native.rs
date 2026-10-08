use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_core::OpCode;

impl ExecCtx {
    pub(super) fn exec_native_op(
        &mut self,
        op: OpCode,
        code: &[u16],
        ip: &mut usize,
        base: usize,
        first_reg: usize,
        closure: &crate::closure::VmClosure,
    ) -> VmResult<()> {
        match op {
            OpCode::Intrinsic => {
                let w1 = code[*ip];
                *ip += 1;
                let wire_byte = (w1 >> 8) as u8;

                let arg_count = (w1 & 0xFF) as usize;

                let args_start = first_reg;

                let result = if arg_count <= 16 {
                    let mut buf = [VmValue::null(); 16];
                    for (i, slot) in buf.iter_mut().take(arg_count).enumerate() {
                        *slot = self.stack.box_reg(base, args_start + i);
                    }
                    (crate::exec::intrinsics::dispatch(wire_byte, &buf[..arg_count]))?
                } else {
                    let boxed = self.stack.box_range(base, args_start, arg_count);
                    (crate::exec::intrinsics::dispatch(wire_byte, &boxed))?
                };
                (self.stack.unbox_into_reg(base, first_reg, result))?;
            }

            OpCode::IntrinsicDirect => {
                let w1 = code[*ip];
                *ip += 1;
                let src = (w1 >> 8) as usize;
                let wire_byte = (w1 & 0xFF) as u8;
                let x = self.stack.box_reg(base, src);
                let result = (crate::exec::intrinsics::dispatch_unary(wire_byte, x))?;
                (self.stack.unbox_into_reg(base, first_reg, result))?;
            }

            OpCode::CallNativeOp => {
                let cidx = code[*ip] as usize;
                let total = code[*ip + 1] as usize;
                *ip += 2;

                let op_id = match closure.proto.chunk.constants.get(cidx) {
                    Some(varn_types::chunk::PoolEntry::Literal(
                        varn_types::chunk::Literal::Int(i),
                    )) => *i as u64,
                    Some(varn_types::chunk::PoolEntry::Literal(varn_types::chunk::Literal::Null | varn_types::chunk::Literal::Bool(_) | varn_types::chunk::Literal::Float(_) | varn_types::chunk::Literal::Str(_) | varn_types::chunk::Literal::BigInt(_) | varn_types::chunk::Literal::Decimal(_) | varn_types::chunk::Literal::Symbol(_) | varn_types::chunk::Literal::Char(_))) | Some(varn_types::chunk::PoolEntry::Function(_)) | Some(varn_types::chunk::PoolEntry::Shape(_)) | Some(varn_types::chunk::PoolEntry::Layout(_)) | None => {
                        return Err(crate::error::RuntimeError::new(format!(
                            "CallNativeOp: const {cidx} is not an op-id"
                        )))
                    }
                };
                let f = (varn_builtins::native_op_fn(op_id).ok_or_else(|| {
                    crate::error::RuntimeError::new(format!("CallNativeOp: unknown op-id {op_id}"))
                }))?;
                let receiver = self.stack.box_reg(base, first_reg);

                let result = (self.call_native_with_receiver(
                    f,
                    receiver,
                    crate::exec::method_args::MethodArgs::Regs {
                        base,
                        start: first_reg + 1,
                        count: total - 1,
                    },
                ))?;
                (self.stack.unbox_into_reg(base, first_reg, result))?;
            }

            OpCode::LoadConst | OpCode::LoadNull | OpCode::LoadTrue | OpCode::LoadFalse | OpCode::LoadInt | OpCode::Move | OpCode::LoadGlobal | OpCode::StoreGlobal | OpCode::DefineGlobal | OpCode::DefineGlobalIdx | OpCode::LoadGlobalIdx | OpCode::StoreGlobalIdx | OpCode::LoadUpvalue | OpCode::StoreUpvalue | OpCode::CloseUpvalue | OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod | OpCode::Pow | OpCode::Negate | OpCode::Not | OpCode::ToString | OpCode::Eq | OpCode::Neq | OpCode::Lt | OpCode::Lte | OpCode::Gt | OpCode::Gte | OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::Shl | OpCode::Shr | OpCode::Ushr | OpCode::Jump | OpCode::JumpIfFalse | OpCode::JumpIfTrue | OpCode::Loop | OpCode::Call | OpCode::CallMethod | OpCode::InvokeVirtual | OpCode::CallSpread | OpCode::Return | OpCode::BuildArray | OpCode::BuildTuple | OpCode::BuildObject | OpCode::BuildObjectWithShape | OpCode::BuildRecord | OpCode::GetIndex | OpCode::SetIndex | OpCode::ObjectRest | OpCode::ObjectKeys | OpCode::ObjectMerge | OpCode::GetProperty | OpCode::GetPropertyMaybe | OpCode::SetProperty | OpCode::GetFixedField | OpCode::SetFixedField | OpCode::GetSuper | OpCode::GetSymbol | OpCode::MakeClosure | OpCode::MakeClass | OpCode::Inherit | OpCode::Method | OpCode::DefineStatic | OpCode::DefineGetter | OpCode::DefineSetter | OpCode::DefineStaticGetter | OpCode::DefineStaticSetter | OpCode::DeclareLayout | OpCode::AllocInstance | OpCode::BindMethod | OpCode::Typeof | OpCode::Instanceof | OpCode::In | OpCode::IsNull | OpCode::IsArray | OpCode::AssertNotNull | OpCode::StrConcat | OpCode::StrLength | OpCode::StrSlice | OpCode::ArrayLength | OpCode::ArrayPush | OpCode::ArrayPop | OpCode::ArrayExtend | OpCode::WrapSpread | OpCode::MakeEnumVariant | OpCode::GetEnumTag | OpCode::Await | OpCode::Spawn | OpCode::Yield | OpCode::Try | OpCode::Throw | OpCode::PopTry | OpCode::LoadModule | OpCode::LoadModuleSlot | OpCode::StoreModuleSlot | OpCode::InvokeRuntimeStatic | OpCode::AddImm | OpCode::SubImm | OpCode::BuildStr | OpCode::LoadIntZero | OpCode::LoadIntOne | OpCode::LoadIntMinusOne | OpCode::AddInt | OpCode::SubInt | OpCode::MulInt | OpCode::DivInt | OpCode::LtInt | OpCode::GtInt | OpCode::LteInt | OpCode::GteInt | OpCode::EqInt | OpCode::NeqInt | OpCode::AddFloat | OpCode::SubFloat | OpCode::MulFloat | OpCode::DivFloat | OpCode::ModFloat | OpCode::PowFloat | OpCode::LtFloat | OpCode::GtFloat | OpCode::LteFloat | OpCode::GteFloat | OpCode::EqFloat | OpCode::NeqFloat | OpCode::ModInt | OpCode::PowInt | OpCode::LoadStaticFn | OpCode::CallSelf | OpCode::Nop | OpCode::ArrayGetIndex | OpCode::ArraySetIndex | OpCode::LoadNativeGlobalIdx | OpCode::BuildMap | OpCode::MapGetIndex | OpCode::MapSetIndex | OpCode::Convert | OpCode::BytesLength => unreachable!("exec_native_op called with a non-native opcode"),
        }
        Ok(())
    }
}
