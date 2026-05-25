use crate::assembler::{Assembler, Reg};
use crate::safepoint::{emit_load_reg, emit_store_reg};
use std::collections::HashMap;
use varn_core::OpCode;

/// Available callee-saved physical registers for virtual reg mapping.
/// On Windows: Rbx, R12-R15 are callee-saved.
/// On Unix: Rbx, R12-R15 are callee-saved.
/// We also avoid ARG_CTX/ARG_CLOSURE/ARG_BASE/ARG_EXEC_CTX, RAX, R10, R11 (scratch).
const ALLOC_REGS: &[Reg] = &[Reg::Rbx, Reg::R12, Reg::R13, Reg::R14, Reg::R15];

/// Maps virtual register index → physical CPU register (if allocated).
pub struct RegMap {
    /// virtual_reg → physical_reg index in ALLOC_REGS
    map: HashMap<usize, Reg>,
    /// which physical regs are actually used (for prologue/epilogue)
    pub used_phys: Vec<Reg>,
}

impl RegMap {
    /// Analyze bytecode and allocate the N hottest virtual registers to physical regs.
    pub fn from_bytecode(code: &[u16]) -> Self {
        let mut freq: HashMap<usize, u32> = HashMap::new();

        let mut ip = 0;
        while ip < code.len() {
            let raw_op = code[ip];
            ip += 1;
            let first_reg = (raw_op >> 8) as usize;
            let op = match OpCode::from_u8(raw_op as u8) {
                Some(o) => o,
                None => break,
            };

            match op {
                OpCode::Return => {
                    let w1 = code[ip];
                    ip += 1;
                    let src = (w1 & 0xFF) as usize;
                    *freq.entry(src).or_insert(0) += 2;
                }
                OpCode::LoadNull
                | OpCode::LoadTrue
                | OpCode::LoadFalse
                | OpCode::LoadIntZero
                | OpCode::LoadIntOne
                | OpCode::LoadIntMinusOne => {
                    *freq.entry(first_reg).or_insert(0) += 1;
                }
                OpCode::LoadInt
                | OpCode::LoadConst
                | OpCode::LoadGlobalIdx
                | OpCode::LoadGlobal
                | OpCode::LoadUpvalue => {
                    *freq.entry(first_reg).or_insert(0) += 2;
                    ip += 1;
                }
                OpCode::StoreGlobalIdx | OpCode::DefineGlobalIdx => {
                    let w1 = code[ip];
                    ip += 1;
                    let src = (w1 >> 8) as usize;
                    *freq.entry(src).or_insert(0) += 2;
                    ip += 1;
                }
                OpCode::StoreUpvalue => {
                    let w1 = code[ip];
                    ip += 1;
                    let src = (w1 & 0xFF) as usize;
                    *freq.entry(src).or_insert(0) += 2;
                }
                OpCode::Move => {
                    let w1 = code[ip];
                    ip += 1;
                    let src = (w1 >> 8) as usize;
                    *freq.entry(first_reg).or_insert(0) += 1;
                    *freq.entry(src).or_insert(0) += 1;
                }
                OpCode::AddImm | OpCode::SubImm => {
                    let w1 = code[ip];
                    ip += 1;
                    let src = (w1 >> 8) as usize;
                    *freq.entry(first_reg).or_insert(0) += 2;
                    *freq.entry(src).or_insert(0) += 2;
                }
                OpCode::ToString
                | OpCode::IsNull
                | OpCode::ArrayLength
                | OpCode::ArrayPush
                | OpCode::ArrayPop
                | OpCode::ArrayExtend
                | OpCode::StrLength => {
                    let w1 = code[ip];
                    ip += 1;
                    let src = (w1 >> 8) as usize;
                    *freq.entry(first_reg).or_insert(0) += 2;
                    *freq.entry(src).or_insert(0) += 2;
                }
                OpCode::MakeClosure => {
                    let w1 = code[ip];
                    ip += 1;
                    let uv_count = (w1 & 0xFF) as usize;
                    ip += 1; // for proto_idx
                    *freq.entry(first_reg).or_insert(0) += 2;
                    for _ in 0..uv_count {
                        let uv_desc = code[ip];
                        ip += 1;
                        let is_local = (uv_desc >> 8) != 0;
                        let index = (uv_desc & 0xFF) as usize;
                        if is_local {
                            *freq.entry(index).or_insert(0) += 2;
                        }
                    }
                }
                OpCode::Eq
                | OpCode::Neq
                | OpCode::Lt
                | OpCode::Lte
                | OpCode::Gt
                | OpCode::Gte
                | OpCode::EqFloat
                | OpCode::NeqFloat
                | OpCode::LtFloat
                | OpCode::LteFloat
                | OpCode::GtFloat
                | OpCode::GteFloat
                | OpCode::Add
                | OpCode::Sub
                | OpCode::Mul
                | OpCode::Div
                | OpCode::Mod
                | OpCode::Pow
                | OpCode::AddInt
                | OpCode::SubInt
                | OpCode::MulInt
                | OpCode::DivInt
                | OpCode::LtInt
                | OpCode::GtInt
                | OpCode::LteInt
                | OpCode::GteInt
                | OpCode::EqInt
                | OpCode::NeqInt
                | OpCode::AddFloat
                | OpCode::SubFloat
                | OpCode::MulFloat
                | OpCode::DivFloat
                | OpCode::GetIndex
                | OpCode::Instanceof
                | OpCode::StrConcat
                | OpCode::StrSlice
                | OpCode::BitAnd
                | OpCode::BitOr
                | OpCode::BitXor
                | OpCode::Shl
                | OpCode::Shr
                | OpCode::Ushr => {
                    let w1 = code[ip];
                    ip += 1;
                    let src1 = (w1 >> 8) as usize;
                    let src2 = (w1 & 0xFF) as usize;
                    *freq.entry(first_reg).or_insert(0) += 2;
                    *freq.entry(src1).or_insert(0) += 2;
                    *freq.entry(src2).or_insert(0) += 2;
                }
                OpCode::SetIndex => {
                    let w1 = code[ip];
                    ip += 1;
                    let idx_reg = (w1 >> 8) as usize;
                    let val_reg = (w1 & 0xFF) as usize;
                    *freq.entry(first_reg).or_insert(0) += 2;
                    *freq.entry(idx_reg).or_insert(0) += 2;
                    *freq.entry(val_reg).or_insert(0) += 2;
                }
                OpCode::Typeof => {
                    let w1 = code[ip];
                    ip += 1;
                    let src = (w1 >> 8) as usize;
                    *freq.entry(first_reg).or_insert(0) += 2;
                    *freq.entry(src).or_insert(0) += 2;
                }
                OpCode::Jump | OpCode::Loop => {
                    ip += 2;
                }
                OpCode::JumpIfFalse | OpCode::JumpIfTrue => {
                    *freq.entry(first_reg).or_insert(0) += 3;
                    ip += 2;
                }
                _ => break,
            }
        }

        // Sort by frequency descending, take top N
        let mut ranked: Vec<(usize, u32)> = freq.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1));

        let n = ALLOC_REGS.len().min(ranked.len());
        let mut map = HashMap::new();
        let mut used_phys = Vec::new();

        for (i, (vreg, _freq)) in ranked.iter().take(n).enumerate() {
            // Only allocate if accessed more than once (avoids wasting a reg on dead stores)
            if ranked[i].1 >= 2 {
                map.insert(*vreg, ALLOC_REGS[i]);
                used_phys.push(ALLOC_REGS[i]);
            }
        }

        Self { map, used_phys }
    }

    #[inline]
    pub fn get(&self, vreg: usize) -> Option<Reg> {
        self.map.get(&vreg).copied()
    }

    pub fn map_iter(&self) -> impl Iterator<Item = (&usize, &Reg)> {
        self.map.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Emit prologue: save callee-saved regs to hardware stack, then load their initial values
/// from the VM stack into the physical registers.
pub fn emit_prologue(asm: &mut Assembler, regmap: &RegMap) {
    // 1. Preserve REG_FRAME_BASE (Rbp)
    asm.push(crate::registers::REG_FRAME_BASE);

    // 2. Initialize REG_FRAME_BASE = ARG_CTX + ARG_BASE * 8
    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_CTX);

    // 3. Save physical registers allocated to virtual registers
    for &phys in &regmap.used_phys {
        asm.push(phys);
    }
}

/// Emit epilogue: flush all allocated physical regs back to VM stack, then restore.
pub fn emit_epilogue(asm: &mut Assembler, regmap: &RegMap) {
    // Flush all physical regs back to their VM stack slots before returning
    for (&vreg, &phys) in &regmap.map {
        emit_store_phys_to_mem(asm, phys, vreg);
    }
    // Restore in reverse order
    for &phys in regmap.used_phys.iter().rev() {
        asm.pop(phys);
    }
    // Restore REG_FRAME_BASE (Rbp)
    asm.pop(crate::registers::REG_FRAME_BASE);
}

/// Load virtual register `vreg` into CPU register `dest`.
/// If `vreg` is mapped to a physical reg, emit a reg-reg move. Otherwise emit a mem load.
#[inline]
pub fn emit_load(asm: &mut Assembler, dest: Reg, vreg: usize, regmap: &RegMap) {
    if let Some(phys) = regmap.get(vreg) {
        if phys != dest {
            asm.mov_reg_reg(dest, phys);
        }
        // if phys == dest, value already in dest — no op
    } else {
        emit_load_reg(asm, dest, vreg);
    }
}

/// Store CPU register `src` into virtual register `vreg`.
/// If `vreg` is mapped to a physical reg, emit a reg-reg move. Otherwise emit a mem store.
#[inline]
pub fn emit_store(asm: &mut Assembler, src: Reg, vreg: usize, regmap: &RegMap) {
    if let Some(phys) = regmap.get(vreg) {
        if phys != src {
            asm.mov_reg_reg(phys, src);
        }
        // if phys == src, already there — no op
    } else {
        emit_store_reg(asm, src, vreg);
    }
}

/// Flush a single physical register back to its VM stack slot.
/// Used at call boundaries (helper calls) to sync state.
pub fn emit_flush_all(asm: &mut Assembler, regmap: &RegMap) {
    for (&vreg, &phys) in &regmap.map {
        emit_store_phys_to_mem(asm, phys, vreg);
    }
}

/// Reload all physical registers from their VM stack slots.
/// Used after helper calls to resync.
pub fn emit_reload_all(asm: &mut Assembler, regmap: &RegMap) {
    for (&vreg, &phys) in &regmap.map {
        emit_load_reg(asm, phys, vreg);
    }
}

/// Reload all physical registers from their VM stack slots, except a specified virtual register.
/// Used after helper calls where the destination register has already been updated.
pub fn emit_reload_all_except(asm: &mut Assembler, regmap: &RegMap, except: Option<usize>) {
    for (&vreg, &phys) in &regmap.map {
        if Some(vreg) != except {
            emit_load_reg(asm, phys, vreg);
        }
    }
}

fn emit_store_phys_to_mem(asm: &mut Assembler, phys: Reg, vreg: usize) {
    emit_store_reg(asm, phys, vreg);
}
