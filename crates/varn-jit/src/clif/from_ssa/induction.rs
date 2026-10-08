use rustc_hash::FxHashSet;

use varn_types::ssa::{SsaBinOp, SsaOp, SsaProto, SsaTerm};

#[derive(Clone, Copy, PartialEq)]
enum Bound {
    Below,

    Above,
}

pub(super) fn in_range_steps(
    ssa: &SsaProto,
    preds: &[Vec<usize>],
    rpo_pos: &[usize],
    reached: &[bool],
) -> FxHashSet<u32> {
    let mut def: Vec<Option<&SsaOp>> = vec![None; ssa.values.len()];
    for inst in ssa.blocks.iter().flat_map(|blk| &blk.insts) {
        if let Some(d) = inst.dest {
            def[d as usize] = Some(&inst.op);
        }
    }
    let is_one = |v: u32| matches!(def[v as usize], Some(SsaOp::ConstInt(1)));

    let mut steps = FxHashSet::default();
    for (header, blk) in ssa.blocks.iter().enumerate() {
        if !reached[header] {
            continue;
        }
        let SsaTerm::Branch {
            cond,
            then_blk,
            else_blk,
            ..
        } = &blk.term
        else {
            continue;
        };

        let in_header = blk.insts.iter().any(|i| i.dest == Some(*cond));
        let Some(SsaOp::Binary { op, lhs, rhs }) = def[*cond as usize] else {
            continue;
        };
        let (k, bound) = match op {
            SsaBinOp::IntLt => (*lhs, Bound::Below),
            SsaBinOp::IntGt => (*lhs, Bound::Above),
            _ => continue,
        };
        let (k, bound) = if blk.params.contains(&k) {
            (k, bound)
        } else {
            let flipped = match bound {
                Bound::Below => Bound::Above,
                Bound::Above => Bound::Below,
            };
            (*rhs, flipped)
        };
        if !in_header || !blk.params.contains(&k) {
            continue;
        }
        let body = super::cfg::loop_body(preds, rpo_pos, reached, header);
        if body.is_empty()
            || !body.contains(&(*then_blk as usize))
            || body.contains(&(*else_blk as usize))
        {
            continue;
        }
        for &b in body.iter().filter(|&&b| b != header) {
            for inst in &ssa.blocks[b].insts {
                let (Some(d), SsaOp::Binary { op, lhs, rhs }) = (inst.dest, &inst.op) else {
                    continue;
                };
                let proven = match (op, bound) {
                    (SsaBinOp::IntAdd, Bound::Below) => {
                        (*lhs == k && is_one(*rhs)) || (*rhs == k && is_one(*lhs))
                    }
                    (SsaBinOp::IntSub, Bound::Above) => *lhs == k && is_one(*rhs),
                    _ => false,
                };
                if proven {
                    steps.insert(d);
                }
            }
        }
    }
    steps
}
