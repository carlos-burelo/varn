use crate::assembler::{Assembler, Reg};
use crate::safepoint::{emit_load_reg, emit_store_reg};
use std::collections::HashMap;
use varn_core::OpCode;

const ALLOC_REGS: &[Reg] = &[Reg::Rbx, Reg::R12, Reg::R13, Reg::R14];

pub struct RegMap {
    map: HashMap<usize, Reg>,
    pub used_phys: Vec<Reg>,
}

impl RegMap {
    pub fn from_bytecode(code: &[u16], constants: &[varn_types::chunk::PoolEntry]) -> Self {
        // Rank virtual registers by access frequency using the shared
        // instruction decoder (varn_types::bytecode). Cheap moves and
        // immediate loads weigh 1, normal defs/uses 2, branch conditions 3.
        let mut freq: HashMap<usize, u32> = HashMap::new();

        let mut ip = 0;
        while ip < code.len() {
            let Some(op) = OpCode::from_u16(code[ip]) else {
                break;
            };
            let Some(info) = varn_types::bytecode::decode(code, ip, constants) else {
                break;
            };

            let def_weight = match op {
                OpCode::LoadNull
                | OpCode::LoadTrue
                | OpCode::LoadFalse
                | OpCode::LoadIntZero
                | OpCode::LoadIntOne
                | OpCode::LoadIntMinusOne
                | OpCode::LoadStaticFn
                | OpCode::Move => 1,
                _ => 2,
            };
            let use_weight = match op {
                OpCode::JumpIfFalse | OpCode::JumpIfTrue => 3,
                OpCode::Move => 1,
                _ => 2,
            };

            if let Some(d) = info.def {
                *freq.entry(d as usize).or_insert(0) += def_weight;
            }
            for u in &info.uses {
                *freq.entry(*u as usize).or_insert(0) += use_weight;
            }

            ip += info.len;
        }

        let mut ranked: Vec<(usize, u32)> = freq.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1));

        let n = ALLOC_REGS.len().min(ranked.len());
        let mut map = HashMap::new();
        let mut used_phys = Vec::new();

        for (i, (vreg, _freq)) in ranked.iter().take(n).enumerate() {
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

pub fn emit_prologue(
    asm: &mut Assembler,
    regmap: &RegMap,
    proto: &varn_types::FunctionProto,
    helpers: &crate::JitHelpers,
) {
    let is_pure = proto.upvalue_count == 0 && !proto.is_async && !proto.is_generator;
    if is_pure {
        use crate::assembler::Cond;

        asm.mov_reg_mem(Reg::R10, crate::registers::ARG_EXEC_CTX, 0);

        asm.mov_reg_reg(Reg::R11, crate::registers::ARG_BASE);
        asm.add_reg_imm32(Reg::R11, (proto.register_count + 32) as i32);

        asm.cmp_reg_reg(Reg::R10, Reg::R11);
        let skip_grow = asm.jmp_cond(Cond::GreaterEqual);

        asm.push(crate::registers::ARG_CTX);
        asm.push(crate::registers::ARG_CLOSURE);
        asm.push(crate::registers::ARG_BASE);
        asm.push(crate::registers::ARG_EXEC_CTX);

        let need_dummy = true;
        if need_dummy {
            asm.push(Reg::Rax);
        }

        asm.mov_reg_reg(crate::registers::ARG_CTX, crate::registers::ARG_EXEC_CTX);

        asm.mov_reg_reg(crate::registers::ARG_CLOSURE, crate::registers::ARG_BASE);
        asm.add_reg_imm32(crate::registers::ARG_CLOSURE, proto.register_count as i32);

        #[cfg(target_os = "windows")]
        asm.add_reg_imm8(Reg::Rsp, -32);

        asm.mov_reg_imm64(Reg::R10, helpers.jit_ensure_stack_capacity as u64);
        asm.call_reg(Reg::R10);

        #[cfg(target_os = "windows")]
        asm.add_reg_imm8(Reg::Rsp, 32);

        if need_dummy {
            asm.pop(Reg::R11);
        }

        asm.pop(crate::registers::ARG_EXEC_CTX);
        asm.pop(crate::registers::ARG_BASE);
        asm.pop(crate::registers::ARG_CLOSURE);
        asm.pop(crate::registers::ARG_CTX);

        asm.mov_reg_mem(crate::registers::ARG_CTX, crate::registers::ARG_EXEC_CTX, 8);

        let post_grow_pos = asm.current_offset();
        let disp = (post_grow_pos as i32 - (skip_grow as i32 + 4)) as u32;
        asm.patch_u32(skip_grow, disp);

        asm.mov_reg_reg(Reg::R10, crate::registers::ARG_BASE);
        asm.add_reg_imm32(Reg::R10, proto.register_count as i32);
        asm.mov_mem_reg(crate::registers::ARG_EXEC_CTX, 16, Reg::R10);
    }

    asm.push(crate::registers::REG_FRAME_BASE);

    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_CTX);

    asm.push(crate::registers::REG_INT_TAG);

    for &phys in &regmap.used_phys {
        asm.push(phys);
    }

    asm.push(crate::registers::ARG_CLOSURE);
    asm.push(Reg::Rax);

    asm.mov_reg_imm64(crate::registers::REG_INT_TAG, 0x7FFC_0000_0000_0000u64);
}

pub fn emit_epilogue(asm: &mut Assembler, regmap: &RegMap) {
    for (&vreg, &phys) in &regmap.map {
        emit_store_phys_to_mem(asm, phys, vreg);
    }

    asm.pop(Reg::Rax);
    asm.pop(crate::registers::ARG_CLOSURE);

    for &phys in regmap.used_phys.iter().rev() {
        asm.pop(phys);
    }

    asm.pop(crate::registers::REG_INT_TAG);

    asm.pop(crate::registers::REG_FRAME_BASE);
}

#[inline]
pub fn emit_load(asm: &mut Assembler, dest: Reg, vreg: usize, regmap: &RegMap) {
    if let Some(phys) = regmap.get(vreg) {
        if phys != dest {
            asm.mov_reg_reg(dest, phys);
        }
    } else {
        emit_load_reg(asm, dest, vreg);
    }
}

#[inline]
pub fn emit_store(asm: &mut Assembler, src: Reg, vreg: usize, regmap: &RegMap) {
    if let Some(phys) = regmap.get(vreg) {
        if phys != src {
            asm.mov_reg_reg(phys, src);
        }
    } else {
        emit_store_reg(asm, src, vreg);
    }
}

pub fn emit_flush_all(asm: &mut Assembler, regmap: &RegMap) {
    for (&vreg, &phys) in &regmap.map {
        emit_store_phys_to_mem(asm, phys, vreg);
    }
}

pub fn emit_reload_all(asm: &mut Assembler, regmap: &RegMap) {
    for (&vreg, &phys) in &regmap.map {
        emit_load_reg(asm, phys, vreg);
    }
}

pub fn emit_reload_all_except(asm: &mut Assembler, regmap: &RegMap, except: Option<usize>) {
    for (&vreg, &phys) in &regmap.map {
        if Some(vreg) != except {
            emit_load_reg(asm, phys, vreg);
        }
    }
}

/// True for register slots whose value is always a NaN-boxed immediate
/// (`int`/`float`/`bool`) and therefore can never be a heap pointer / GC root.
#[inline]
fn slot_is_immediate(meta: &[varn_types::register_meta::RegisterMeta], vreg: usize) -> bool {
    use varn_types::register_meta::SlotKind;
    // Missing metadata (e.g. protos deserialized from the bytecode cache,
    // where `register_meta` is #[serde(skip)]) must be treated as
    // pointer-bearing: skipping the flush for an unknown slot hides live heap
    // references from the GC.
    meta.get(vreg).map_or(false, |m| {
        matches!(m.kind, SlotKind::Int | SlotKind::Float | SlotKind::Bool)
    })
}

/// Flush enregistered locals to their VM-stack slots ahead of a call.
///
/// When `skip_immediates` holds (caller proven free of local-capture aliasing),
/// registers holding immediate values that are *not* call arguments are left
/// only in their callee-saved physical register: they can never be GC roots and
/// the callee never reads their stack slot, so the spill is dead. Arguments in
/// `[arg_lo, arg_hi)` are always flushed — the callee reads them from memory —
/// and pointer-bearing slots are always flushed so the GC can see them.
pub fn emit_flush_for_call(
    asm: &mut Assembler,
    regmap: &RegMap,
    meta: &[varn_types::register_meta::RegisterMeta],
    skip_immediates: bool,
    arg_lo: usize,
    arg_hi: usize,
) {
    for (&vreg, &phys) in &regmap.map {
        if skip_immediates && !(arg_lo..arg_hi).contains(&vreg) && slot_is_immediate(meta, vreg) {
            continue;
        }
        emit_store_phys_to_mem(asm, phys, vreg);
    }
}

/// Reload enregistered locals after a call, mirroring [`emit_flush_for_call`].
///
/// Pointer-bearing slots are always reloaded: a minor (copying) GC during the
/// callee may relocate the object and update the spilled slot, so the stale
/// pointer in the callee-saved register must be refreshed. Immediate slots are
/// skipped under `skip_immediates`: the callee-saved register still holds the
/// exact value, so re-reading memory is dead work.
pub fn emit_reload_after_call(
    asm: &mut Assembler,
    regmap: &RegMap,
    meta: &[varn_types::register_meta::RegisterMeta],
    skip_immediates: bool,
    except: Option<usize>,
) {
    for (&vreg, &phys) in &regmap.map {
        if Some(vreg) == except {
            continue;
        }
        if skip_immediates && slot_is_immediate(meta, vreg) {
            continue;
        }
        emit_load_reg(asm, phys, vreg);
    }
}

fn emit_store_phys_to_mem(asm: &mut Assembler, phys: Reg, vreg: usize) {
    emit_store_reg(asm, phys, vreg);
}
