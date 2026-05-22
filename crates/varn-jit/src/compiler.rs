use varn_core::OpCode;
use varn_types::FunctionProto;

use crate::assembler::{Assembler, Reg};
use crate::mem::JitBuffer;
use crate::safepoint::{emit_load_reg, emit_store_reg};

/// Compiles a `FunctionProto` bytecode function into native x86_64 machine code.
/// Returns a `JitBuffer` containing the executable pages on success, or a bailout error on failure.
pub fn compile_proto(proto: &FunctionProto) -> Result<JitBuffer, String> {
    // 1. Audit opcodes to verify if we support all of them.
    // If we find any unsupported opcode, we perform an eager bailout to the interpreter.
    let code = &proto.chunk.code;
    let mut ip = 0;
    while ip < code.len() {
        let raw_op = code[ip];
        ip += 1;
        let op = OpCode::from_u8(raw_op as u8)
            .ok_or_else(|| format!("Unknown opcode: {}", raw_op))?;

        match op {
            OpCode::Return => {
                // Return has 1 extra word operand in bytecode
                ip += 1;
            }
            OpCode::LoadNull
            | OpCode::LoadTrue
            | OpCode::LoadFalse
            | OpCode::LoadIntZero
            | OpCode::LoadIntOne
            | OpCode::LoadIntMinusOne => {
                // These have no extra operand words
            }
            OpCode::LoadInt => {
                // LoadInt has 1 extra word operand (immediate)
                ip += 1;
            }
            OpCode::Move => {
                // Move has 1 extra word operand
                ip += 1;
            }
            _ => {
                return Err(format!(
                    "JIT Bailout: Opcode '{:?}' is not supported in the JIT compiler",
                    op
                ));
            }
        }
    }

    // 2. Main Compilation Pass
    let mut asm = Assembler::new();
    let mut ip = 0;
    let code_len = code.len();

    // Map from bytecode offset to assembler instruction byte offset
    let mut ip_to_asm_pos = vec![0; code_len + 1];

    while ip < code_len {
        ip_to_asm_pos[ip] = asm.current_offset();

        let raw_op = code[ip];
        ip += 1;
        let first_reg = (raw_op >> 8) as usize;
        let op = OpCode::from_u8(raw_op as u8).unwrap();

        match op {
            OpCode::LoadNull => {
                let null_val = 0x7FF9_0000_0000_0000u64;
                asm.mov_reg_imm64(Reg::Rax, null_val);
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::LoadTrue => {
                let true_val = 0x7FFB_0000_0000_0000u64;
                asm.mov_reg_imm64(Reg::Rax, true_val);
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::LoadFalse => {
                let false_val = 0x7FFA_0000_0000_0000u64;
                asm.mov_reg_imm64(Reg::Rax, false_val);
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::LoadInt => {
                let val = code[ip] as i16;
                ip += 1;
                let int_val = 0x7FFC_0000_0000_0000u64 | (val as u64 & 0x0000_FFFF_FFFF_FFFFu64);
                asm.mov_reg_imm64(Reg::Rax, int_val);
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::LoadIntZero => {
                let int_val = 0x7FFC_0000_0000_0000u64;
                asm.mov_reg_imm64(Reg::Rax, int_val);
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::LoadIntOne => {
                let int_val = 0x7FFC_0000_0000_0001u64;
                asm.mov_reg_imm64(Reg::Rax, int_val);
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::LoadIntMinusOne => {
                let int_val = 0x7FFC_FFFF_FFFF_FFFFu64;
                asm.mov_reg_imm64(Reg::Rax, int_val);
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::Move => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 >> 8) as usize;
                emit_load_reg(&mut asm, Reg::Rax, src);
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::Return => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 & 0xFF) as usize;
                emit_load_reg(&mut asm, Reg::Rax, src);
                asm.ret();
            }
            _ => unreachable!(),
        }
    }

    ip_to_asm_pos[code_len] = asm.current_offset();

    // 3. Assemble and mark executable
    let native_bytes = asm.into_bytes();
    let mut jit_buf = JitBuffer::new(native_bytes.len())?;
    jit_buf.as_mut_slice()[..native_bytes.len()].copy_from_slice(&native_bytes);
    jit_buf.make_executable()?;

    Ok(jit_buf)
}
