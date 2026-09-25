//! Loop counters whose step cannot overflow.
//!
//! `k + 1` normally carries the `int` overflow guard. It cannot overflow when
//! `k` is a loop header's parameter and the header enters the loop only on
//! `k < limit` (the other edge leaving it): every block of the body — each
//! reached from the header only through that edge — runs with
//! `k < limit <= INT_MAX`, so `k + 1 <= INT_MAX`. `k - 1` under `k > limit`
//! is the mirror case. Such a step is a plain add; nothing else changes.

use std::collections::HashSet;

use varn_types::ssa::{SsaBinOp, SsaOp, SsaProto, SsaTerm};

/// Which way a guard bounds the header parameter.
#[derive(Clone, Copy, PartialEq)]
enum Bound {
    /// `k < limit`: `k + 1` stays in range.
    Below,
    /// `k > limit`: `k - 1` stays in range.
    Above,
}

/// The values defined by an `int` step that cannot overflow.
pub(super) fn in_range_steps(
    ssa: &SsaProto,
    preds: &[Vec<usize>],
    rpo_pos: &[usize],
    reached: &[bool],
) -> HashSet<u32> {
    let mut def: Vec<Option<&SsaOp>> = vec![None; ssa.values.len()];
    for inst in ssa.blocks.iter().flat_map(|blk| &blk.insts) {
        if let Some(d) = inst.dest {
            def[d as usize] = Some(&inst.op);
        }
    }
    let is_one = |v: u32| matches!(def[v as usize], Some(SsaOp::ConstInt(1)));

    let mut steps = HashSet::new();
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
        // The guard must be computed in the header itself, on its parameter.
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
            // `limit > k` is `k < limit`, and `limit < k` is `k > limit`.
            let flipped = match bound {
                Bound::Below => Bound::Above,
                Bound::Above => Bound::Below,
            };
            (*rhs, flipped)
        };
        if !in_header || !blk.params.contains(&k) {
            continue;
        }
        let body = loop_body(preds, rpo_pos, reached, header);
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

/// The natural loop headed by `header`: the header and every reached block
/// that reaches one of its back edges' latches without passing it. Empty
/// when no back edge enters `header`, or when the body has another entry — a
/// block other than the header with a predecessor outside it — since the
/// guard then no longer covers every path into the body.
fn loop_body(
    preds: &[Vec<usize>],
    rpo_pos: &[usize],
    reached: &[bool],
    header: usize,
) -> HashSet<usize> {
    let live = |p: &usize| reached[*p];
    let latches: Vec<usize> = preds[header]
        .iter()
        .filter(|p| live(p))
        .copied()
        .filter(|&p| rpo_pos[header] <= rpo_pos[p])
        .collect();
    let mut body = HashSet::new();
    if latches.is_empty() {
        return body;
    }
    body.insert(header);
    let mut stack = latches;
    while let Some(b) = stack.pop() {
        if body.insert(b) {
            stack.extend(preds[b].iter().filter(|p| live(p)).copied());
        }
    }
    let single_entry = body.iter().filter(|&&b| b != header).all(|&b| {
        preds[b]
            .iter()
            .filter(|p| live(p))
            .all(|p| body.contains(p))
    });
    if !single_entry {
        body.clear();
    }
    body
}
