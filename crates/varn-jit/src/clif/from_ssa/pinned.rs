use std::collections::{BTreeMap, BTreeSet};

use varn_types::ssa::{SsaOp, SsaProto};

pub(super) fn compute(
    ssa: &SsaProto,
    preds: &[Vec<usize>],
    rpo_pos: &[usize],
    reached: &[bool],
) -> BTreeMap<usize, Vec<u32>> {
    let mut def_block: BTreeMap<u32, usize> = BTreeMap::new();
    for (b, blk) in ssa.blocks.iter().enumerate() {
        if !reached[b] {
            continue;
        }
        for inst in &blk.insts {
            if let Some(d) = inst.dest {
                def_block.insert(d, b);
            }
        }
    }
    let mut readers: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (b, blk) in ssa.blocks.iter().enumerate() {
        if !reached[b] {
            continue;
        }
        for inst in &blk.insts {
            if let SsaOp::ArrayGetIndex { object, .. } = &inst.op {
                readers.entry(*object).or_default().push(b);
            }
        }
    }
    let mut headers: Vec<usize> = Vec::new();
    for h in 0..ssa.blocks.len() {
        if !reached[h] {
            continue;
        }
        let body = super::cfg::loop_body(preds, rpo_pos, reached, h);
        if !body.is_empty() {
            headers.push(h);
        }
    }
    let mut bodies: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for h in &headers {
        let body = super::cfg::loop_body(preds, rpo_pos, reached, *h);
        bodies.insert(*h, body.into_iter().collect());
    }
    let mut best: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
    for (object, uses) in &readers {
        for h in &headers {
            let body = &bodies[h];
            if !uses.iter().all(|u| body.contains(u)) {
                continue;
            }
            let mut ok = true;
            for b in body.iter() {
                let blk = &ssa.blocks[*b];
                if blk.params.contains(object) {
                    ok = false;
                    break;
                }
                for inst in &blk.insts {
                    if !super::views::keeps_views(ssa, &inst.op) {
                        ok = false;
                        break;
                    }
                    match &inst.op {
                        SsaOp::ArraySetIndex { object: o, .. }
                        | SsaOp::ArrayPush { array: o, .. }
                        | SsaOp::ArrayExtend { array: o, .. } => {
                            if o == object {
                                ok = false;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if !ok {
                    break;
                }
            }
            if !ok {
                continue;
            }
            match def_block.get(object) {
                Some(db) if body.contains(db) => continue,
                None => continue,
                _ => {}
            }
            let outside: Vec<usize> = preds[*h]
                .iter()
                .copied()
                .filter(|p| reached[*p] && !body.contains(p))
                .collect();
            if outside.len() != 1 {
                continue;
            }
            let pre = outside[0];
            let size = body.len();
            match best.get(object) {
                Some((_, s)) if *s >= size => {}
                _ => {
                    best.insert(*object, (pre, size));
                }
            }
        }
    }
    let mut pins: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
    for (object, (pre, _)) in best {
        pins.entry(pre).or_default().push(object);
    }
    for v in pins.values_mut() {
        v.sort_unstable();
        v.dedup();
    }
    pins
}
