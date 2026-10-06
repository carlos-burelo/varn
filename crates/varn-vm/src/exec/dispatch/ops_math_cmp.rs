
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
            _ => return Ok(false),
        }

        Ok(true)
    }
}
