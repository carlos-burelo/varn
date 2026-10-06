use rustc_hash::FxHashSet as HashSet;

use varn_core::OpCode;

use crate::bytecode::decode;
use crate::chunk::PoolEntry;

pub fn collect_back_edges(code: &[u16], constants: &[PoolEntry]) -> Vec<(usize, usize)> {
    let mut word_to_instr: rustc_hash::FxHashMap<usize, usize> = rustc_hash::FxHashMap::default();
    let mut offset = 0usize;
    let mut instr_idx = 0usize;
    while offset < code.len() {
        word_to_instr.insert(offset, instr_idx);
        match decode(code, offset, constants) {
            Some(info) => {
                offset += info.len;
                instr_idx += 1;
            }
            None => break,
        }
    }

    let mut edges = Vec::new();
    let mut offset = 0usize;
    let mut instr_idx = 0usize;
    while offset < code.len() {
        if OpCode::from_u16(code[offset]) == Some(OpCode::Loop) {
            let hi = code.get(offset + 1).copied().unwrap_or(0) as usize;
            let lo = code.get(offset + 2).copied().unwrap_or(0) as usize;
            let back_offset = (hi << 16) | lo;
            let target_word = (offset + 3).saturating_sub(back_offset);
            let target_instr = word_to_instr.get(&target_word).copied().unwrap_or(0);
            edges.push((target_instr, instr_idx));
        }
        match decode(code, offset, constants) {
            Some(info) => {
                offset += info.len;
                instr_idx += 1;
            }
            None => break,
        }
    }
    edges
}

pub struct NaturalLoop {
    pub header: usize,
    pub latch: usize,

    def_set: HashSet<u8>,

    pub has_calls: bool,

    pub mutates_arrays: bool,
}

impl NaturalLoop {
    pub fn is_invariant(&self, reg: u8) -> bool {
        !self.def_set.contains(&reg)
    }
}

pub fn instr_offsets(code: &[u16], constants: &[PoolEntry]) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut offset = 0usize;
    while offset < code.len() {
        offsets.push(offset);
        match decode(code, offset, constants) {
            Some(info) => offset += info.len,
            None => break,
        }
    }
    offsets
}

pub fn natural_loops(code: &[u16], constants: &[PoolEntry]) -> Vec<NaturalLoop> {
    let back_edges = collect_back_edges(code, constants);
    if back_edges.is_empty() {
        return Vec::new();
    }

    let instr_offsets = instr_offsets(code, constants);

    back_edges
        .into_iter()
        .map(|(header, latch)| {
            let mut def_set = HashSet::default();
            let mut has_calls = false;
            let mut mutates_arrays = false;
            for &instr_offset in instr_offsets.iter().take(latch + 1).skip(header) {
                let Some(info) = decode(code, instr_offset, constants) else {
                    continue;
                };
                if let Some(d) = info.def {
                    def_set.insert(d);
                }
                if info.call_args.is_some() {
                    has_calls = true;
                }
                if let Some(op) = OpCode::from_u16(code[instr_offset]) {
                    if matches!(
                        op,
                        OpCode::ArrayPush | OpCode::ArrayPop | OpCode::ArrayExtend
                    ) {
                        mutates_arrays = true;
                    }
                }
            }
            NaturalLoop {
                header,
                latch,
                def_set,
                has_calls,
                mutates_arrays,
            }
        })
        .collect()
}
