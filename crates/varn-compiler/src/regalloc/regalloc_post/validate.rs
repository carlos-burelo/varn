use rustc_hash::FxHashMap as HashMap;
use varn_types::bytecode::layout;
use varn_types::chunk::PoolEntry;

use super::scan::ScanResult;
use crate::regalloc::liveness::LiveRange;




pub(crate) fn verify_interference(ranges: &[LiveRange], mapping: &HashMap<u8, u8>) -> bool {
    let m = |r: u8| mapping.get(&r).copied().unwrap_or(r);
    ranges.iter().all(|range| {
        let color = m(range.vreg as u8);
        range.interference.iter().all(|&n| m(n as u8) != color)
    })
}



pub(crate) fn verify_run_constraints(
    code: &[u16],
    constants: &[PoolEntry],
    mapping: &HashMap<u8, u8>,
) -> bool {
    let m = |r: u8| mapping.get(&r).copied().unwrap_or(r);
    let mut offset = 0;
    while let Some(l) = layout(code, offset, constants) {
        let contiguous = l.runs(code, offset).all(|(start, count, _)| {
            (1..count).all(|i| m(start.wrapping_add(i as u8)) == m(start).wrapping_add(i as u8))
        });
        if !contiguous {
            return false;
        }
        offset += l.len;
    }
    true
}

pub(crate) fn verify_callee_frame_constraints(
    scan: &ScanResult,
    mapping: &HashMap<u8, u8>,
) -> bool {
    let m = |r: u8| mapping.get(&r).copied().unwrap_or(r);
    for &(call_idx, arg_start, arg_count) in &scan.call_sites {
        let mapped_start = m(arg_start);
        let arg_end = arg_start.wrapping_add(arg_count);
        for (&reg, defs) in &scan.defs {
            if defs.first >= call_idx {
                continue;
            }
            if reg >= arg_start && reg < arg_end {
                continue;
            }
            let live_across = scan
                .uses
                .get(&reg)
                .is_some_and(|us| us.iter().any(|&u| u > call_idx));
            if live_across && m(reg) >= mapped_start {
                return false;
            }
        }
    }
    true
}
