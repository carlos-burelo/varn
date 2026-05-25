use crate::assembler::{Assembler, Reg};
use crate::registers::REG_FRAME_BASE;

pub fn emit_load_reg(asm: &mut Assembler, dest_cpu: Reg, virtual_reg: usize) {
    let displacement = (virtual_reg * 8) as i32;
    asm.mov_reg_mem(dest_cpu, REG_FRAME_BASE, displacement);
}

pub fn emit_store_reg(asm: &mut Assembler, src_cpu: Reg, virtual_reg: usize) {
    let displacement = (virtual_reg * 8) as i32;
    asm.mov_mem_reg(REG_FRAME_BASE, displacement, src_cpu);
}
