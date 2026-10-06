use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::chunk::PoolEntry;
use varn_types::loop_analysis::{instr_offsets, natural_loops, NaturalLoop};

#[cfg(target_os = "windows")]
const MAX_CANDIDATES: usize = 2;
#[cfg(not(target_os = "windows"))]
const MAX_CANDIDATES: usize = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CacheSource {
    RegisterInvariant,
    GlobalInvariant(u16),
}

pub struct HoistCandidate {
    pub obj_vreg: u8,
    pub source: CacheSource,
}

fn header_reachable_by_fallthrough(code: &[u16], offsets: &[usize], header_instr: usize) -> bool {
    let Some(prev_instr) = header_instr.checked_sub(1) else {
        return true;
    };
    let prev_offset = offsets[prev_instr];
    !matches!(
        OpCode::from_u16(code[prev_offset]),
        Some(OpCode::Jump | OpCode::Loop | OpCode::Return | OpCode::Throw | OpCode::Yield)
    )
}

pub fn is_alloc_free_op(op: OpCode) -> bool {
    matches!(
        op,
        OpCode::LoadNull
            | OpCode::LoadTrue
            | OpCode::LoadFalse
            | OpCode::LoadInt
            | OpCode::LoadIntZero
            | OpCode::LoadIntOne
            | OpCode::LoadIntMinusOne
            | OpCode::LoadConst
            | OpCode::LoadGlobalIdx
            | OpCode::LoadNativeGlobalIdx
            | OpCode::StoreGlobalIdx
            | OpCode::DefineGlobalIdx
            | OpCode::Move
            | OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Mod
            | OpCode::Pow
            | OpCode::Negate
            | OpCode::Not
            | OpCode::AddImm
            | OpCode::SubImm
            | OpCode::AddInt
            | OpCode::SubInt
            | OpCode::MulInt
            | OpCode::DivInt
            | OpCode::ModInt
            | OpCode::PowInt
            | OpCode::AddFloat
            | OpCode::SubFloat
            | OpCode::MulFloat
            | OpCode::DivFloat
            | OpCode::ModFloat
            | OpCode::PowFloat
            | OpCode::BitAnd
            | OpCode::BitOr
            | OpCode::BitXor
            | OpCode::Shl
            | OpCode::Shr
            | OpCode::Ushr
            | OpCode::Eq
            | OpCode::Neq
            | OpCode::Lt
            | OpCode::Lte
            | OpCode::Gt
            | OpCode::Gte
            | OpCode::LtInt
            | OpCode::GtInt
            | OpCode::LteInt
            | OpCode::GteInt
            | OpCode::EqInt
            | OpCode::NeqInt
            | OpCode::LtFloat
            | OpCode::GtFloat
            | OpCode::LteFloat
            | OpCode::GteFloat
            | OpCode::EqFloat
            | OpCode::NeqFloat
            | OpCode::IsNull
            | OpCode::Jump
            | OpCode::JumpIfFalse
            | OpCode::JumpIfTrue
            | OpCode::Loop
            | OpCode::Return
            | OpCode::GetIndex
            | OpCode::ArrayGetIndex
            | OpCode::ArrayLength
            | OpCode::BytesLength
            | OpCode::SetIndex
            | OpCode::ArraySetIndex
    )
}

fn body_is_alloc_free(code: &[u16], offsets: &[usize], lp: &NaturalLoop) -> bool {
    (lp.header..=lp.latch)
        .all(|instr_idx| OpCode::from_u16(code[offsets[instr_idx]]).is_some_and(is_alloc_free_op))
}

fn global_stored_in_range(
    code: &[u16],
    constants: &[PoolEntry],
    start_offset: usize,
    end_offset: usize,
    target_idx: u16,
) -> bool {
    let mut off = start_offset;
    while off < end_offset {
        let Some(op) = OpCode::from_u16(code[off]) else {
            return true;
        };
        let Some(info) = decode(code, off, constants) else {
            return true;
        };
        if matches!(op, OpCode::StoreGlobalIdx | OpCode::DefineGlobalIdx)
            && code[off + 2] == target_idx
        {
            return true;
        }
        off += info.len;
    }
    false
}

fn classify_site(
    code: &[u16],
    constants: &[PoolEntry],
    header_offset: usize,
    latch_end_offset: usize,
    site_offset: usize,
    obj_reg: u8,
) -> Option<CacheSource> {
    let mut last_def: Option<(OpCode, usize)> = None;
    let mut off = header_offset;
    while off < site_offset {
        let op = OpCode::from_u16(code[off])?;
        let info = decode(code, off, constants)?;
        if info.def == Some(obj_reg) {
            last_def = Some((op, off));
        }
        off += info.len;
    }
    match last_def {
        None => Some(CacheSource::RegisterInvariant),
        Some((OpCode::LoadGlobalIdx, def_off)) => {
            let idx = code[def_off + 1];
            if global_stored_in_range(code, constants, header_offset, latch_end_offset, idx) {
                None
            } else {
                Some(CacheSource::GlobalInvariant(idx))
            }
        }
        Some(_) => None,
    }
}

pub struct LoopDiagnostic {
    pub header_offset: usize,
    pub latch_offset: usize,

    pub is_real: bool,

    pub is_alloc_free: bool,

    pub is_innermost: bool,

    pub candidates: Vec<HoistCandidate>,
}

pub fn diagnose_loops(code: &[u16], constants: &[PoolEntry]) -> Vec<LoopDiagnostic> {
    let loops = natural_loops(code, constants);
    if loops.is_empty() {
        return Vec::new();
    }
    let offsets = instr_offsets(code, constants);

    let is_real: Vec<bool> = loops
        .iter()
        .map(|lp| header_reachable_by_fallthrough(code, &offsets, lp.header))
        .collect();

    loops
        .iter()
        .enumerate()
        .filter_map(|(i, lp)| {
            let alloc_free = body_is_alloc_free(code, &offsets, lp);
            let is_innermost = !loops.iter().enumerate().any(|(j, other)| {
                j != i && is_real[j] && other.header >= lp.header && other.latch <= lp.latch
            });

            let header_offset = offsets[lp.header];
            let latch_offset = offsets[lp.latch];
            let latch_end_offset = latch_offset + decode(code, latch_offset, constants)?.len;

            let mut candidates: Vec<HoistCandidate> = Vec::with_capacity(MAX_CANDIDATES);
            if is_real[i] && alloc_free && is_innermost {
                for &instr_offset in &offsets[lp.header..=lp.latch] {
                    let op = OpCode::from_u16(code[instr_offset])?;
                    if !matches!(op, OpCode::ArrayGetIndex | OpCode::ArrayLength) {
                        continue;
                    }
                    let info = decode(code, instr_offset, constants)?;
                    let obj_reg = *info.uses.first()?;

                    let source = if lp.is_invariant(obj_reg) {
                        CacheSource::RegisterInvariant
                    } else {
                        match classify_site(
                            code,
                            constants,
                            header_offset,
                            latch_end_offset,
                            instr_offset,
                            obj_reg,
                        ) {
                            Some(s) => s,
                            None => continue,
                        }
                    };

                    if candidates
                        .iter()
                        .any(|c| c.obj_vreg == obj_reg && c.source == source)
                    {
                        continue;
                    }

                    if candidates.len() >= MAX_CANDIDATES {
                        continue;
                    }
                    candidates.push(HoistCandidate {
                        obj_vreg: obj_reg,
                        source,
                    });
                }
            }

            Some(LoopDiagnostic {
                header_offset,
                latch_offset,
                is_real: is_real[i],
                is_alloc_free: alloc_free,
                is_innermost,
                candidates,
            })
        })
        .collect()
}
