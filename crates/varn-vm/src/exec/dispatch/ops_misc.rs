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

            _ => unreachable!("exec_misc_op called with a non-misc opcode"),
        }
        Ok(())
    }
}
