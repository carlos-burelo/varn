use super::range_math::{I64_HI, I64_LO};
use varn_types::register_meta::SlotKind;
use varn_types::ssa::{SsaBinOp, SsaOp, SsaProto, SsaTerm};

pub(super) type Fact = (u32, i128, i128);

pub(super) fn immediate_dominators(
    preds: &[Vec<usize>],
    rpo_pos: &[usize],
    reached: &[bool],
) -> Vec<Option<usize>> {
    let mut order: Vec<usize> = (0..preds.len()).filter(|&b| reached[b]).collect();
    order.sort_by_key(|&b| rpo_pos[b]);
    let mut idom: Vec<Option<usize>> = vec![None; preds.len()];
    let Some(&entry) = order.first() else {
        return idom;
    };
    idom[entry] = Some(entry);
    let mut changed = true;
    while changed {
        changed = false;
        for &b in &order[1..] {
            let mut next: Option<usize> = None;
            for &p in preds[b]
                .iter()
                .filter(|&&p| reached[p] && idom[p].is_some())
            {
                next = Some(match next {
                    None => p,
                    Some(q) => meet(&idom, rpo_pos, p, q),
                });
            }
            if next != idom[b] {
                idom[b] = next;
                changed = true;
            }
        }
    }
    idom
}

fn meet(idom: &[Option<usize>], rpo_pos: &[usize], mut a: usize, mut b: usize) -> usize {
    while a != b {
        while rpo_pos[a] > rpo_pos[b] {
            a = idom[a].unwrap_or(a);
        }
        while rpo_pos[b] > rpo_pos[a] {
            b = idom[b].unwrap_or(b);
        }
    }
    a
}

fn compare_fact(
    ssa: &SsaProto,
    def: &[Option<&SsaOp>],
    consts: &dyn Fn(u32) -> Option<i128>,
    cond: u32,
    on_then: bool,
) -> Option<Fact> {
    let Some(SsaOp::Binary { op, lhs, rhs }) = def[cond as usize] else {
        return None;
    };
    let (var, bound, op) = match (consts(*lhs), consts(*rhs)) {
        (None, Some(c)) => (*lhs, c, *op),
        (Some(c), None) => (
            *rhs,
            c,
            match op {
                SsaBinOp::IntLt => SsaBinOp::IntGt,
                SsaBinOp::IntLe => SsaBinOp::IntGe,
                SsaBinOp::IntGt => SsaBinOp::IntLt,
                SsaBinOp::IntGe => SsaBinOp::IntLe,
                other @ (SsaBinOp::IntAdd
                | SsaBinOp::IntSub
                | SsaBinOp::IntMul
                | SsaBinOp::IntDiv
                | SsaBinOp::IntMod
                | SsaBinOp::IntPow
                | SsaBinOp::IntEq
                | SsaBinOp::IntNe
                | SsaBinOp::IntAnd
                | SsaBinOp::IntOr
                | SsaBinOp::IntXor
                | SsaBinOp::IntShl
                | SsaBinOp::IntShr
                | SsaBinOp::IntUshr
                | SsaBinOp::FloatAdd
                | SsaBinOp::FloatSub
                | SsaBinOp::FloatMul
                | SsaBinOp::FloatDiv
                | SsaBinOp::FloatMod
                | SsaBinOp::FloatPow
                | SsaBinOp::FloatEq
                | SsaBinOp::FloatNe
                | SsaBinOp::FloatLt
                | SsaBinOp::FloatLe
                | SsaBinOp::FloatGt
                | SsaBinOp::FloatGe
                | SsaBinOp::StrConcat
                | SsaBinOp::StrEq
                | SsaBinOp::StrNe
                | SsaBinOp::Dyn(_)) => *other,
            },
        ),
        _ => return None,
    };
    if ssa.value_ty(var) != SlotKind::Int {
        return None;
    }
    let (lo, hi) = match (op, on_then) {
        (SsaBinOp::IntLt, true) | (SsaBinOp::IntGe, false) => (I64_LO, bound - 1),
        (SsaBinOp::IntLe, true) | (SsaBinOp::IntGt, false) => (I64_LO, bound),
        (SsaBinOp::IntGt, true) | (SsaBinOp::IntLe, false) => (bound + 1, I64_HI),
        (SsaBinOp::IntGe, true) | (SsaBinOp::IntLt, false) => (bound, I64_HI),
        _ => return None,
    };
    Some((var, lo, hi))
}

pub(super) fn per_block_facts(
    ssa: &SsaProto,
    def: &[Option<&SsaOp>],
    preds: &[Vec<usize>],
    rpo_pos: &[usize],
    reached: &[bool],
    consts: &dyn Fn(u32) -> Option<i128>,
) -> Vec<Vec<Fact>> {
    let idom = immediate_dominators(preds, rpo_pos, reached);
    let mut order: Vec<usize> = (0..ssa.blocks.len()).filter(|&b| reached[b]).collect();
    order.sort_by_key(|&b| rpo_pos[b]);
    let mut facts: Vec<Vec<Fact>> = vec![Vec::new(); ssa.blocks.len()];
    for &b in &order {
        let mut mine = match idom[b] {
            Some(d) if d != b => facts[d].clone(),
            _ => Vec::new(),
        };
        let live: Vec<usize> = preds[b].iter().copied().filter(|&p| reached[p]).collect();
        if let [p] = live.as_slice()
            && let SsaTerm::Branch {
                cond,
                then_blk,
                else_blk,
                ..
            } = &ssa.blocks[*p].term
            && then_blk != else_blk
            && ssa.blocks[*p].insts.iter().any(|i| i.dest == Some(*cond))
        {
            let on_then = *then_blk as usize == b;
            if let Some(f) = compare_fact(ssa, def, consts, *cond, on_then) {
                mine.push(f);
            }
        }
        facts[b] = mine;
    }
    facts
}

pub(super) fn refine(
    facts: &[Fact],
    value: u32,
    known: Option<(i128, i128)>,
) -> Option<(i128, i128)> {
    let mut range = known;
    for &(v, lo, hi) in facts {
        if v != value {
            continue;
        }
        let (cl, ch) = range.unwrap_or((I64_LO, I64_HI));
        let (nl, nh) = (cl.max(lo), ch.min(hi));
        if nl > nh {
            return range;
        }
        range = Some((nl, nh));
    }
    range
}
