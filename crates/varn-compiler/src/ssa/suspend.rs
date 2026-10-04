//! Localiza los puntos de suspensión de una función y qué los cruza.
//!
//! Sólo lectura: no transforma el IR. El pase de máquinas de estados consume
//! este análisis en vez de recalcularlo, para que "qué es un punto de
//! suspensión" y "qué debe guardar el estado" tengan una sola definición.

use super::ir::{InstKind, SsaFunc, Value};
use super::liveness::Liveness;

#[derive(Debug, Clone)]
pub struct SuspendPoint {
    pub operand: Value,
    pub live: Vec<Value>,
}

pub fn analyze(ssa: &SsaFunc) -> Vec<SuspendPoint> {
    let lv = Liveness::analyze(ssa);
    let mut out = Vec::new();

    for (b, block) in ssa.blocks.iter().enumerate() {
        for (i, inst) in block.insts.iter().enumerate() {
            let operand = match &inst.kind {
                InstKind::Await { operand } | InstKind::Yield { operand } => *operand,
                _ => continue,
            };
            out.push(SuspendPoint {
                operand,
                live: lv.live_after(ssa, b, i),
            });
        }
    }
    out
}

/// Live physical registers per suspension resume point, sorted by resume
/// ip. Same `live_after` sets as [`analyze`], mapped through `reg`, plus
/// every `Try` handler's `live_in`: an exception delivered into a parked
/// frame resumes at a handler that no CFG edge from the suspend point
/// reaches, so handler-read values must survive every park. Omits a point
/// if any value lacks a register: absence means full roots.
pub fn suspend_live_regs(
    ssa: &SsaFunc,
    reg: &[u8],
    inst_next: &[Vec<usize>],
) -> Vec<(u32, Vec<u16>)> {
    let lv = Liveness::analyze(ssa);
    let handlers = try_handlers(ssa);
    let mut table = Vec::new();
    for (b, block) in ssa.blocks.iter().enumerate() {
        for (i, inst) in block.insts.iter().enumerate() {
            match &inst.kind {
                InstKind::Await { .. } | InstKind::Yield { .. } => {}
                _ => continue,
            }
            let live = resume_live(ssa, &lv, &handlers, b, i);
            let mut regs: Vec<u16> = Vec::with_capacity(live.len());
            let mut ok = true;
            for v in &live {
                match reg.get(v.0 as usize) {
                    Some(&r) => regs.push(r as u16),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                continue;
            }
            regs.push(0);
            let ip = inst_next
                .get(b)
                .and_then(|row| row.get(i))
                .copied()
                .unwrap_or(usize::MAX);
            let Ok(resume_ip) = u32::try_from(ip) else {
                continue;
            };
            if resume_ip == u32::MAX {
                continue;
            }
            regs.sort_unstable();
            regs.dedup();
            table.push((resume_ip, regs));
        }
    }
    table.sort_by_key(|(ip, _)| *ip);
    table
}

/// Every block a `Try` hands control to.
pub fn try_handlers(ssa: &SsaFunc) -> Vec<usize> {
    let mut handlers: Vec<usize> = Vec::new();
    for block in &ssa.blocks {
        for inst in &block.insts {
            if let InstKind::Try { handler } = &inst.kind {
                let h = handler.0 as usize;
                if !handlers.contains(&h) {
                    handlers.push(h);
                }
            }
        }
    }
    handlers
}

/// The values the interpreter reads when it resumes after instruction `i` of
/// block `b`: those live after it, plus every handler's `live_in`, since an
/// exception delivered into a parked frame resumes at a handler no CFG edge
/// from the suspension reaches. Sorted, deduplicated.
pub fn resume_live(
    ssa: &SsaFunc,
    lv: &Liveness,
    handlers: &[usize],
    b: usize,
    i: usize,
) -> Vec<Value> {
    let mut live = lv.live_after(ssa, b, i);
    for &h in handlers {
        if let Some(live_in) = lv.live_in.get(h) {
            for &v in live_in {
                live.push(Value(v));
            }
        }
    }
    live.sort_unstable_by_key(|v| v.0);
    live.dedup();
    live
}
