use rustc_hash::FxHashMap as HashMap;
use varn_core::OpCode;
use varn_types::bytecode::{decode, layout};
use varn_types::chunk::PoolEntry;

use crate::regalloc::liveness::DefSites;

pub(crate) struct ScanResult {
    pub(crate) defs: HashMap<u8, DefSites>,
    pub(crate) uses: HashMap<u8, Vec<usize>>,
    pub(crate) call_sites: Vec<(usize, u8, u8)>,
}

pub(crate) fn scan_bytecode(code: &[u16], constants: &[PoolEntry]) -> ScanResult {
    let mut defs: HashMap<u8, DefSites> = HashMap::default();
    let mut uses: HashMap<u8, Vec<usize>> = HashMap::default();
    let mut call_sites: Vec<(usize, u8, u8)> = Vec::new();
    let mut open_captures: Vec<u8> = Vec::new();

    let mut offset = 0;
    let mut instr_idx = 0usize;

    while offset < code.len() {
        let info = match decode(code, offset, constants) {
            Some(i) => i,
            None => break,
        };

        // A captured register is read for as long as the closure lives.
        if let Some(l) = layout(code, offset, constants).filter(|l| l.op == OpCode::MakeClosure) {
            open_captures.extend(l.read_registers(code, offset));
        }

        if info.opaque {
            offset += info.len;
            instr_idx += 1;
            continue;
        }

        if let Some(def_reg) = info.def {
            defs.entry(def_reg)
                .and_modify(|d| d.extend(instr_idx))
                .or_insert_with(|| DefSites::at(instr_idx));
        }
        for &use_reg in &info.uses {
            uses.entry(use_reg).or_default().push(instr_idx);
        }
        if let Some((arg_start, arg_count)) = info.call_args {
            call_sites.push((instr_idx, arg_start, arg_count));
        }

        offset += info.len;
        instr_idx += 1;
    }

    let last = instr_idx.saturating_sub(1);
    for reg in open_captures {
        uses.entry(reg).or_default().push(last);
    }

    ScanResult {
        defs,
        uses,
        call_sites,
    }
}

/// Every run of registers an instruction reads together — a call's
/// arguments, a collection's elements — as `(first, count)`, when it spans
/// more than one register: the colouring must keep each contiguous.
pub(crate) fn collect_consecutive_blocks(code: &[u16], constants: &[PoolEntry]) -> Vec<(u8, u8)> {
    let mut blocks = Vec::new();
    let mut offset = 0;
    while let Some(l) = layout(code, offset, constants) {
        blocks.extend(
            l.runs(code, offset)
                .filter(|&(_, count, _)| count > 1)
                .map(|(start, count, _)| (start, count as u8)),
        );
        offset += l.len;
    }
    blocks
}
