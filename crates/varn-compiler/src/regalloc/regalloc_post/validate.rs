use std::collections::HashMap;
use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::chunk::PoolEntry;

use crate::regalloc::liveness::LiveRange;
use super::scan::ScanResult;

/// The colouring contract, re-checked against the mapping that is about to be
/// written: two registers whose live ranges overlap must not land on the same
/// physical register.
pub(crate) fn verify_interference(ranges: &[LiveRange], mapping: &HashMap<u8, u8>) -> bool {
    let m = |r: u8| mapping.get(&r).copied().unwrap_or(r);
    ranges.iter().all(|range| {
        let color = m(range.vreg as u8);
        range.interference.iter().all(|&n| m(n as u8) != color)
    })
}

pub(crate) fn verify_call_constraints(
    code: &[u16],
    constants: &[PoolEntry],
    mapping: &HashMap<u8, u8>,
) -> bool {
    let mut offset = 0;
    while offset < code.len() {
        let info = match decode(code, offset, constants) {
            Some(i) => i,
            None => break,
        };
        if let Some((arg_start, arg_count)) = info.call_args {
            let mapped_start = match mapping.get(&arg_start) {
                Some(&m) => m,
                None => arg_start,
            };
            for i in 1..arg_count {
                let orig = arg_start.wrapping_add(i);
                let mapped = mapping.get(&orig).copied().unwrap_or(orig);
                if mapped != mapped_start.wrapping_add(i) {
                    return false;
                }
            }
        }
        offset += info.len;
    }
    true
}

pub(crate) fn verify_callee_frame_constraints(scan: &ScanResult, mapping: &HashMap<u8, u8>) -> bool {
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

pub(crate) fn verify_build_array_constraints(
    code: &[u16],
    constants: &[PoolEntry],
    mapping: &HashMap<u8, u8>,
) -> bool {
    let mut offset = 0;
    while offset < code.len() {
        let info = match decode(code, offset, constants) {
            Some(i) => i,
            None => break,
        };

        if let Some(op) = OpCode::from_u16(code[offset]) {
            if matches!(op, OpCode::BuildArray | OpCode::BuildTuple) {
                let w1 = if offset + 1 < code.len() {
                    code[offset + 1]
                } else {
                    0
                };
                let w2 = if offset + 2 < code.len() {
                    code[offset + 2]
                } else {
                    0
                };
                let start = (w1 & 0xff) as u8;
                let count = (w2 >> 8) as u8;
                if count > 1 {
                    let mapped_start = mapping.get(&start).copied().unwrap_or(start);
                    for i in 1..count {
                        let orig = start.wrapping_add(i);
                        let mapped = mapping.get(&orig).copied().unwrap_or(orig);
                        if mapped != mapped_start.wrapping_add(i) {
                            return false;
                        }
                    }
                }
            }
        }

        offset += info.len;
    }
    true
}

pub(crate) fn verify_build_object_with_shape_constraints(
    code: &[u16],
    constants: &[PoolEntry],
    mapping: &HashMap<u8, u8>,
) -> bool {
    let mut offset = 0;
    while offset < code.len() {
        let info = match decode(code, offset, constants) {
            Some(i) => i,
            None => break,
        };

        if let Some(op) = OpCode::from_u16(code[offset]) {
            if matches!(op, OpCode::BuildObjectWithShape | OpCode::BuildRecord) {
                let w1 = if offset + 1 < code.len() {
                    code[offset + 1]
                } else {
                    0
                };
                let w2 = if offset + 2 < code.len() {
                    code[offset + 2]
                } else {
                    0
                };
                let start = (w1 & 0xff) as u8;
                let shape_idx = w2 as usize;
                let count = match constants.get(shape_idx) {
                    Some(PoolEntry::Shape(k)) => k.len(),
                    _ => 0,
                };
                if count > 1 {
                    let mapped_start = mapping.get(&start).copied().unwrap_or(start);
                    for i in 1..count {
                        let orig = start.wrapping_add(i as u8);
                        let mapped = mapping.get(&orig).copied().unwrap_or(orig);
                        if mapped != mapped_start.wrapping_add(i as u8) {
                            return false;
                        }
                    }
                }
            }
        }

        offset += info.len;
    }
    true
}
