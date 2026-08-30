use std::cell::Cell;
use std::collections::HashMap;
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

thread_local! {
    pub static OPTIMIZE_TIME: Cell<Duration> = const { Cell::new(Duration::ZERO) };
    pub static OPTIMIZE_ENABLED: Cell<bool> = const { Cell::new(true) };
}


pub fn optimize_function(proto: &mut FunctionProto) {
    let start = if OPTIMIZE_ENABLED.with(|e| e.get()) {
        Some(Instant::now())
    } else {
        None
    };

    optimize_function_inner(proto);

    if let Some(start) = start {
        let elapsed = start.elapsed();
        OPTIMIZE_TIME.with(|t| t.set(t.get() + elapsed));
    }
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

    // The SSA allocator (ssa/emit) keeps float and non-float values in
    // separate registers so the backend can route native f64. This coalescing
    // pass re-colours by liveness alone, so it could re-pack a float register
    // with a non-float one — meeting `register_meta` to Dynamic and erasing the
    // float type. Skip the whole pass for any function that owns a float
    // register: it keeps the segregated allocation at the cost of not
    // coalescing that function's Moves. Pure-int functions are unaffected.
    if proto
        .register_meta
        .iter()
        .any(|m| m.kind == varn_types::register_meta::SlotKind::Float)
    {
        return;
    }

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
    // `scan.defs` es un `std::collections::HashMap`, así que usa `RandomState`:
    // su orden de iteración se siembra al azar en CADA arranque de proceso.
    //
    // Ese orden no se queda aquí. Llega intacto a `ranges` (el analizador
    // recorre `vregs_used` en orden y empuja los `LiveRange` en ese mismo
    // orden), y `color_with_base` ordena con `sort_by`, que es ESTABLE y sólo
    // desempata por `start`. Dos vregs definidos en el mismo punto empatan, el
    // empate conserva el orden de entrada, y quien va primero se lleva el color
    // más bajo.
    //
    // Resultado sin este `sort`: dos compilaciones del mismo binario sobre la
    // misma fuente producen bytecode con los registros físicos permutados
    // (mismos opcodes, mismo recuento). Ordenar hace la asignación reproducible.
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
                let w1 = proto.chunk.code[offset + 1];
                let dest = (proto.chunk.code[offset] >> 8) as u8;
                let src = (w1 >> 8) as u8;
                if dest >= base && src >= base {
                    copies.push((dest, src));
                }
            }
            offset += info.len;
        } else {
            break;
        }
    }
    let blocks = collect_consecutive_blocks(&proto.chunk.code, &proto.chunk.constants);
    let raw_mapping = match color_with_base(&ranges, base, &copies, &scan, &blocks) {
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

    if !verify_call_constraints(&proto.chunk.code, &proto.chunk.constants, &mapping) {
        return;
    }

    if !verify_callee_frame_constraints(&scan, &mapping) {
        return;
    }

    if !verify_build_array_constraints(&proto.chunk.code, &proto.chunk.constants, &mapping) {
        return;
    }

    if !verify_build_object_with_shape_constraints(
        &proto.chunk.code,
        &proto.chunk.constants,
        &mapping,
    ) {
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

    // register_meta was derived per pre-coalescing register (ssa/emit);
    // permute it through the same mapping, meeting kinds when two old
    // registers merge into one.
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
}
