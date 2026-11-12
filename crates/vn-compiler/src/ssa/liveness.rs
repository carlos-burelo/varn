use super::ir::{InstKind, SsaFunc, Terminator, Value};
use rustc_hash::FxHashSet;

#[derive(Debug)]
pub struct Liveness {
    pub def: Vec<u32>,

    pub end: Vec<u32>,

    pub live_out: Vec<FxHashSet<u32>>,

    pub live_in: Vec<FxHashSet<u32>>,
}

impl Liveness {
    pub fn analyze(ssa: &SsaFunc) -> Liveness {
        let order: Vec<usize> = (0..ssa.blocks.len()).collect();
        Self::analyze_ordered(ssa, &order)
    }

    pub fn analyze_ordered(ssa: &SsaFunc, order: &[usize]) -> Liveness {
        let nvals = ssa.values.len();
        let nblocks = ssa.blocks.len();
        let mut def = vec![u32::MAX; nvals];
        let mut last = vec![0u32; nvals];
        let mut term_idx = vec![0u32; nblocks];
        let mut defs: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
        let mut uses: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
        let mut succ: Vec<Vec<usize>> = vec![Vec::new(); nblocks];
        let mut idx = 0u32;
        for &b in order {
            let block = &ssa.blocks[b];
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
        for &b in order {
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
            live_out,
            live_in,
        }
    }

    pub fn live_after(&self, ssa: &SsaFunc, b: usize, i: usize) -> Vec<Value> {
        let block = &ssa.blocks[b];
        debug_assert!(
            i < block.insts.len(),
            "live_after: i {i} fuera de rango para b{b} ({} insts)",
            block.insts.len()
        );
        let mut live: FxHashSet<u32> = self.live_out[b].clone();

        for u in crate::ssa::verify::term_value_uses(&block.term) {
            live.insert(u.0);
        }
        for (_, args) in crate::ssa::verify::out_edges(&block.term) {
            for a in args {
                live.insert(a.0);
            }
        }

        for inst in block.insts[i + 1..].iter().rev() {
            if let Some(d) = inst.dest {
                live.remove(&d.0);
            }
            for u in crate::ssa::verify::inst_uses(&inst.kind) {
                live.insert(u.0);
            }
        }

        let mut out: Vec<Value> = live.into_iter().map(Value).collect();
        out.sort_unstable_by_key(|v| v.0);
        out
    }
}
