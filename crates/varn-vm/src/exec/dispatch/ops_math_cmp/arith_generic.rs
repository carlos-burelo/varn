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
            _ => unreachable!(),
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
            _ => unreachable!(),
        };
        self.stack.unbox_into_reg(base, first_reg, r)?;
        Ok(())
    }
}
