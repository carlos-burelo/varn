use crate::assembler::{Assembler, Reg};
use crate::registers::{ARG_CTX, ARG_BASE, REG_SCRATCH_2};

/// Generates x86_64 instructions to load a VmValue from a VM stack slot (base + r) into a CPU register.
/// Assumes `ARG_CTX` is `stack_ptr` (*mut VmValue) directly.
pub fn emit_load_reg(asm: &mut Assembler, dest_cpu: Reg, virtual_reg: usize) {
    // 1. REG_SCRATCH_2 = ARG_BASE
    asm.mov_reg_reg(REG_SCRATCH_2, ARG_BASE);

    // 2. REG_SCRATCH_2 += virtual_reg
    if virtual_reg > 0 {
        if virtual_reg <= 127 {
            asm.add_reg_imm8(REG_SCRATCH_2, virtual_reg as i8);
        } else {
            asm.add_reg_imm32(REG_SCRATCH_2, virtual_reg as i32);
        }
    }

    // 3. REG_SCRATCH_2 *= 8 (convert index to byte offset)
    asm.shl_reg_imm8(REG_SCRATCH_2, 3);

    // 4. REG_SCRATCH_2 += ARG_CTX (absolute memory address of the stack slot)
    asm.add_reg_reg(REG_SCRATCH_2, ARG_CTX);

    // 5. dest_cpu = *REG_SCRATCH_2
    asm.mov_reg_mem(dest_cpu, REG_SCRATCH_2, 0);
}

/// Generates x86_64 instructions to store a VmValue from a CPU register into a VM stack slot (base + r).
/// Assumes `ARG_CTX` is `stack_ptr` (*mut VmValue) directly.
pub fn emit_store_reg(asm: &mut Assembler, src_cpu: Reg, virtual_reg: usize) {
    // 1. REG_SCRATCH_2 = ARG_BASE
    asm.mov_reg_reg(REG_SCRATCH_2, ARG_BASE);

    // 2. REG_SCRATCH_2 += virtual_reg
    if virtual_reg > 0 {
        if virtual_reg <= 127 {
            asm.add_reg_imm8(REG_SCRATCH_2, virtual_reg as i8);
        } else {
            asm.add_reg_imm32(REG_SCRATCH_2, virtual_reg as i32);
        }
    }

    // 3. REG_SCRATCH_2 *= 8
    asm.shl_reg_imm8(REG_SCRATCH_2, 3);

    // 4. REG_SCRATCH_2 += ARG_CTX
    asm.add_reg_reg(REG_SCRATCH_2, ARG_CTX);

    // 5. *REG_SCRATCH_2 = src_cpu
    asm.mov_mem_reg(REG_SCRATCH_2, 0, src_cpu);
}

