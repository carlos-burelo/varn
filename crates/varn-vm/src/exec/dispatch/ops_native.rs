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
                    _ => {
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

            _ => unreachable!("exec_native_op called with a non-native opcode"),
        }
        Ok(())
    }
}
