//! Natural-loop analysis shared by backend register allocation (live-range
//! widening across back-edges) and the JIT's loop-invariant guard hoisting.
//! Back-edge detection lives here ONLY — see [`crate::bytecode::decode`]'s
//! docstring: every walker over `chunk.code` advances with the shared
//! decoder, and loop-boundary detection is no exception.

use std::collections::HashSet;

use varn_core::OpCode;

use crate::bytecode::decode;
use crate::chunk::PoolEntry;

/// `(header_instr, latch_instr)` pairs, one per `Loop` opcode. Both are
/// instruction indices (not code-word offsets).
pub fn collect_back_edges(code: &[u16], constants: &[PoolEntry]) -> Vec<(usize, usize)> {
    let mut word_to_instr: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::new();
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

/// A reducible natural loop: body = instructions `[header, latch]`
/// (inclusive), one back-edge per header — Varn's loop emitter never
/// produces irreducible control flow.
pub struct NaturalLoop {
    pub header: usize,
    pub latch: usize,
    /// Registers written anywhere in the body. A register NOT in this set
    /// is loop-invariant: every iteration observes the same value.
    def_set: HashSet<u8>,
    /// Body contains a call-shaped instruction. A call may trigger GC,
    /// which can move or promote a heap object — any cached raw pointer
    /// into the heap must not survive one.
    pub has_calls: bool,
    /// Body contains `ArrayPush`/`ArrayPop`/`ArrayExtend` — array length or
    /// backing capacity may change, invalidating a cached length or a
    /// cached payload pointer (capacity growth reallocates).
    pub mutates_arrays: bool,
}

impl NaturalLoop {
    pub fn is_invariant(&self, reg: u8) -> bool {
        !self.def_set.contains(&reg)
    }
}

/// Code-word offset of every instruction, indexed by instruction number —
/// the shared `instr_idx -> offset` table walkers need to translate
/// [`NaturalLoop`]'s instruction-index bounds into `chunk.code` offsets.
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

/// Natural loops in `code`, built from [`collect_back_edges`] plus a
/// per-body scan for def-set / call / array-mutation facts.
pub fn natural_loops(code: &[u16], constants: &[PoolEntry]) -> Vec<NaturalLoop> {
    let back_edges = collect_back_edges(code, constants);
    if back_edges.is_empty() {
        return Vec::new();
    }

    let instr_offsets = instr_offsets(code, constants);

    back_edges
        .into_iter()
        .map(|(header, latch)| {
            let mut def_set = HashSet::new();
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
