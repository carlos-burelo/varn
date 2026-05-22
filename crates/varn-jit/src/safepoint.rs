use crate::assembler::{Assembler, Reg};
use crate::registers::REG_FRAME_BASE;

/// Generates x86_64 instructions to load a VmValue from a VM stack slot (base + r) into a CPU register.
/// Assumes `REG_FRAME_BASE` points directly to the first virtual register slot of the current frame stack.
pub fn emit_load_reg(asm: &mut Assembler, dest_cpu: Reg, virtual_reg: usize) {
    let displacement = (virtual_reg * 8) as i32;
    asm.mov_reg_mem(dest_cpu, REG_FRAME_BASE, displacement);
}

/// Generates x86_64 instructions to store a VmValue from a CPU register into a VM stack slot (base + r).
/// Assumes `REG_FRAME_BASE` points directly to the first virtual register slot of the current frame stack.
pub fn emit_store_reg(asm: &mut Assembler, src_cpu: Reg, virtual_reg: usize) {
    let displacement = (virtual_reg * 8) as i32;
    asm.mov_mem_reg(REG_FRAME_BASE, displacement, src_cpu);
}

