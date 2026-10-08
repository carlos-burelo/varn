use rustc_hash::FxHashMap as HashMap;
use std::time::{Duration, Instant};
use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::FunctionProto;

pub mod color;
pub mod rewrite;
pub mod scan;
pub mod validate;

pub(crate) use color::*;
pub(crate) use rewrite::*;
pub(crate) use scan::*;
pub(crate) use validate::*;

use crate::regalloc::liveness::LivenessAnalyzer;

pub fn optimize_function(proto: &mut FunctionProto, measure: bool) -> Duration {
    let start = if measure { Some(Instant::now()) } else { None };

    optimize_function_inner(proto);

    start.map(|s| s.elapsed()).unwrap_or_default()
}

fn optimize_function_inner(proto: &mut FunctionProto) {
    if proto.is_async || proto.is_generator {
        return;
    }

    let fixed_count = proto.arity + if proto.has_this { 1 } else { 0 };
    let base = fixed_count as u8;
    if proto.chunk.code.is_empty() {
        return;
    }

    let kinds: Vec<varn_types::register_meta::SlotKind> = (0..proto.register_count as usize)
        .map(|r| {
            proto
                .register_meta
                .get(r)
                .map(|m| m.kind)
                .unwrap_or(varn_types::register_meta::SlotKind::Dynamic)
        })
        .collect();

    let back_edges =
        varn_types::loop_analysis::collect_back_edges(&proto.chunk.code, &proto.chunk.constants);
    let scan = scan_bytecode(&proto.chunk.code, &proto.chunk.constants);

    let mut analyzer = LivenessAnalyzer::new();
    for (&reg, defs) in &scan.defs {
        if reg >= base {
            analyzer.record_def(reg as u16, defs.first);
            analyzer.record_def(reg as u16, defs.last);
        }
    }
    for (&reg, use_positions) in &scan.uses {
        if reg >= base {
            for &pos in use_positions {
                analyzer.record_use(reg as u16, pos);
            }
        }
    }

    for &reg in scan.uses.keys() {
        if reg >= base && !scan.defs.contains_key(&reg) {
            analyzer.record_def(reg as u16, 0);
        }
    }

    let mut all_regs: Vec<u16> = scan
        .defs
        .keys()
        .filter(|&&r| r >= base)
        .map(|&r| r as u16)
        .collect();

    all_regs.sort_unstable();

    if all_regs.is_empty() {
        return;
    }

    let ranges = analyzer.analyze_with_back_edges(all_regs, &back_edges);
    if ranges.is_empty() {
        return;
    }

    let mut copies = Vec::new();
    let mut offset = 0;
    while offset < proto.chunk.code.len() {
        if let Some(info) = decode(&proto.chunk.code, offset, &proto.chunk.constants) {
            if OpCode::from_u16(proto.chunk.code[offset]) == Some(OpCode::Move) {
                if let (Some(dest), Some(&src)) = (info.def, info.uses.first()) {
                    if dest >= base && src >= base {
                        copies.push((dest, src));
                    }
                }
            }
            offset += info.len;
        } else {
            break;
        }
    }
    let blocks = collect_consecutive_blocks(&proto.chunk.code, &proto.chunk.constants);
    let raw_mapping = match color_with_base(&ranges, base, &copies, &scan, &blocks, &kinds) {
        Some(m) => m,
        None => return,
    };

    let mapping: HashMap<u8, u8> = raw_mapping
        .into_iter()
        .filter(|&(old, new)| old != new)
        .collect();

    if mapping.is_empty() {
        return;
    }

    if !verify_interference(&ranges, &mapping) {
        return;
    }

    if !verify_run_constraints(&proto.chunk.code, &proto.chunk.constants, &mapping) {
        return;
    }

    if !verify_callee_frame_constraints(&scan, &mapping) {
        return;
    }

    let new_max = scan
        .defs
        .keys()
        .map(|&r| mapping.get(&r).copied().unwrap_or(r))
        .chain(
            scan.uses
                .keys()
                .map(|&r| mapping.get(&r).copied().unwrap_or(r)),
        )
        .max()
        .unwrap_or(0);
    let new_register_count = new_max as u16 + 1;

    remap_bytecode(&mut proto.chunk.code, &proto.chunk.constants, &mapping);

    if new_register_count < proto.register_count {
        proto.register_count = new_register_count;
    }

    if !proto.register_meta.is_empty() {
        use varn_types::register_meta::{RegisterMeta, SlotKind};
        let mut merged: Vec<Option<SlotKind>> = vec![None; new_register_count as usize];
        for (old, meta) in proto.register_meta.iter().enumerate() {
            let old8 = old as u8;
            let new = mapping.get(&old8).copied().unwrap_or(old8) as usize;
            let Some(slot) = merged.get_mut(new) else {
                continue;
            };
            *slot = Some(match *slot {
                None => meta.kind,
                Some(cur) if cur == meta.kind => cur,
                Some(_) => SlotKind::Dynamic,
            });
        }
        proto.register_meta = merged
            .into_iter()
            .map(|k| RegisterMeta {
                kind: k.unwrap_or(SlotKind::Dynamic),
            })
            .collect();
    }

    proto.register_count = new_register_count;

    if let Some(ssa) = proto.ssa.get_mut() {
        ssa.map_registers(|r| {
            let old = r as u8;
            mapping.get(&old).copied().unwrap_or(old) as u32
        });
        ssa.register_count = proto.register_count;
    }
}
