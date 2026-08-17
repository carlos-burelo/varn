//! Liveness sobre valores SSA: único dueño del dataflow.
//!
//! Dos consumidores: la asignación de registros (`emit::regs`) y el pase de
//! máquinas de estados. Tenerlo dos veces es como se separan.
//!
//! `end` es un intervalo LINEAL, no liveness exacta por punto: para un valor
//! definido dentro de un bucle y usado en la cabecera, el intervalo abarca
//! todo el bucle. Es una sobre-aproximación conservadora — nunca declara
//! muerto algo vivo — así que es correcta para asignar registros y correcta
//! para elegir campos de estado, a costa de guardar de más en bucles.

use super::ir::{InstKind, SsaFunc, Terminator, Value};
use rustc_hash::FxHashSet;

pub struct Liveness {
    pub def: Vec<u32>,
    pub end: Vec<u32>,
    pub live_in: Vec<FxHashSet<u32>>,
    pub live_out: Vec<FxHashSet<u32>>,
    pub term_idx: Vec<u32>,
}

impl Liveness {
    pub fn analyze(ssa: &SsaFunc) -> Liveness {
        let nvals = ssa.values.len();
        let nblocks = ssa.blocks.len();
        let mut def = vec![u32::MAX; nvals];
        let mut last = vec![0u32; nvals];
        let mut term_idx = vec![0u32; nblocks];
        let mut defs: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
        let mut uses: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
        let mut succ: Vec<Vec<usize>> = vec![Vec::new(); nblocks];
        let mut idx = 0u32;
        for (b, block) in ssa.blocks.iter().enumerate() {
            let mut local_defined: FxHashSet<u32> = FxHashSet::default();
            for p in &block.params {
                if def[p.0 as usize] == u32::MAX {
                    def[p.0 as usize] = idx;
                }
                defs[b].insert(p.0);
                local_defined.insert(p.0);
            }
            idx += 1;
            for inst in &block.insts {
                for u in crate::ssa::verify::inst_uses(&inst.kind) {
                    if last[u.0 as usize] < idx {
                        last[u.0 as usize] = idx;
                    }
                    if !local_defined.contains(&u.0) {
                        uses[b].insert(u.0);
                    }
                }
                if let Some(d) = inst.dest {
                    if def[d.0 as usize] == u32::MAX {
                        def[d.0 as usize] = idx;
                    }
                    defs[b].insert(d.0);
                    local_defined.insert(d.0);
                }
                if let InstKind::Try { handler } = &inst.kind {
                    succ[b].push(handler.0 as usize);
                }
                idx += 1;
            }
            let mut touch = |v: Value, uses: &mut FxHashSet<u32>| {
                if last[v.0 as usize] < idx {
                    last[v.0 as usize] = idx;
                }
                if !local_defined.contains(&v.0) {
                    uses.insert(v.0);
                }
            };
            match &block.term {
                Terminator::Return(Some(v)) | Terminator::Throw(v) => touch(*v, &mut uses[b]),
                Terminator::Branch {
                    cond,
                    then_blk,
                    then_args,
                    else_blk,
                    else_args,
                } => {
                    touch(*cond, &mut uses[b]);
                    then_args
                        .iter()
                        .chain(else_args)
                        .for_each(|a| touch(*a, &mut uses[b]));
                    succ[b].push(then_blk.0 as usize);
                    succ[b].push(else_blk.0 as usize);
                }
                Terminator::Jump { target, args } => {
                    args.iter().for_each(|a| touch(*a, &mut uses[b]));
                    succ[b].push(target.0 as usize);
                }
                Terminator::Return(None) | Terminator::Unreachable => {}
            }
            term_idx[b] = idx;
            idx += 1;
        }

        let mut live_in: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
        let mut live_out: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
        let mut changed = true;
        while changed {
            changed = false;
            for b in (0..nblocks).rev() {
                let mut out = FxHashSet::default();
                for &s in &succ[b] {
                    out.extend(live_in[s].iter().copied());
                }
                let mut nin = uses[b].clone();
                nin.extend(out.iter().copied().filter(|v| !defs[b].contains(v)));
                if out != live_out[b] || nin != live_in[b] {
                    live_out[b] = out;
                    live_in[b] = nin;
                    changed = true;
                }
            }
        }

        let mut end = last;
        for b in 0..nblocks {
            for &v in &live_out[b] {
                if end[v as usize] < term_idx[b] {
                    end[v as usize] = term_idx[b];
                }
            }
        }
        for v in 0..nvals {
            if def[v] != u32::MAX && end[v] < def[v] {
                end[v] = def[v];
            }
        }

        Liveness {
            def,
            end,
            live_in,
            live_out,
            term_idx,
        }
    }

    /// Valores vivos cruzando el punto `idx`: definidos antes y todavía vivos
    /// después. Es la consulta que necesita el pase de máquinas de estados
    /// para decidir qué guarda el objeto de estado.
    pub fn live_across(&self, idx: u32) -> Vec<u32> {
        (0..self.def.len() as u32)
            .filter(|&v| {
                let d = self.def[v as usize];
                d != u32::MAX && d < idx && self.end[v as usize] > idx
            })
            .collect()
    }
}
