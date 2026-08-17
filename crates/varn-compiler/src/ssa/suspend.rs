//! Localiza los puntos de suspensión de una función y qué los cruza.
//!
//! Sólo lectura: no transforma el IR. El pase de máquinas de estados consume
//! este análisis en vez de recalcularlo, para que "qué es un punto de
//! suspensión" y "qué debe guardar el estado" tengan una sola definición.

use super::ir::{InstKind, SsaFunc, Terminator, Value};
use super::liveness::Liveness;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuspendKind {
    Await,
    Yield,
}

#[derive(Debug, Clone)]
pub struct SuspendPoint {
    pub block: usize,
    pub inst: usize,
    pub kind: SuspendKind,
    pub operand: Value,
    pub dest: Option<Value>,
    /// Valores vivos cruzando el punto: los campos que necesitará el objeto de
    /// estado. Ordenado por índice de valor.
    pub live: Vec<Value>,
    pub in_try: bool,
    pub in_loop: bool,
}

pub fn analyze(ssa: &SsaFunc) -> Vec<SuspendPoint> {
    let lv = Liveness::analyze(ssa);
    let in_loop = compute_in_loop(ssa);
    let mut out = Vec::new();

    for (b, block) in ssa.blocks.iter().enumerate() {
        // Un `Try` abre cobertura para el resto del bloque; basta con contar
        // los que se han visto antes de la instrucción actual.
        let mut try_depth = 0usize;
        for (i, inst) in block.insts.iter().enumerate() {
            let kind = match &inst.kind {
                InstKind::Await { .. } => Some(SuspendKind::Await),
                InstKind::Yield { .. } => Some(SuspendKind::Yield),
                _ => None,
            };
            if let Some(kind) = kind {
                let operand = match &inst.kind {
                    InstKind::Await { operand } | InstKind::Yield { operand } => *operand,
                    _ => unreachable!("kind ya filtró a Await/Yield"),
                };
                out.push(SuspendPoint {
                    block: b,
                    inst: i,
                    kind,
                    operand,
                    dest: inst.dest,
                    live: lv.live_after(ssa, b, i),
                    in_try: try_depth > 0,
                    in_loop: in_loop[b],
                });
            }
            if matches!(inst.kind, InstKind::Try { .. }) {
                try_depth += 1;
            }
        }
    }
    out
}

/// `true` para los bloques alcanzables desde sí mismos, o sea los que están en
/// un ciclo del CFG. Es la definición que le importa al pase: un estado situado
/// en un bloque así es reentrable.
fn compute_in_loop(ssa: &SsaFunc) -> Vec<bool> {
    let n = ssa.blocks.len();
    let mut reach = vec![vec![false; n]; n];
    for (b, block) in ssa.blocks.iter().enumerate() {
        for s in succs(block) {
            reach[b][s] = true;
        }
    }
    // Clausura transitiva (Floyd-Warshall booleano). Los CFG de una función
    // caben de sobra en O(n^3) a estos tamaños.
    for k in 0..n {
        for i in 0..n {
            if reach[i][k] {
                for j in 0..n {
                    if reach[k][j] {
                        reach[i][j] = true;
                    }
                }
            }
        }
    }
    (0..n).map(|b| reach[b][b]).collect()
}

fn succs(block: &super::ir::Block) -> Vec<usize> {
    let mut v = Vec::new();
    for inst in &block.insts {
        if let InstKind::Try { handler } = &inst.kind {
            v.push(handler.0 as usize);
        }
    }
    match &block.term {
        Terminator::Jump { target, .. } => v.push(target.0 as usize),
        Terminator::Branch {
            then_blk, else_blk, ..
        } => {
            v.push(then_blk.0 as usize);
            v.push(else_blk.0 as usize);
        }
        Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => {}
    }
    v
}
