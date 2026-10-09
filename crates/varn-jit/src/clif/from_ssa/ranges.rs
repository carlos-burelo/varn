use rustc_hash::{FxHashMap, FxHashSet};
use varn_types::register_meta::SlotKind;
use varn_types::ssa::{SsaBinOp, SsaOp, SsaProto, SsaTerm};

const I64_LO: i128 = i64::MIN as i128;
const I64_HI: i128 = i64::MAX as i128;

fn def_map(ssa: &SsaProto) -> Vec<Option<&SsaOp>> {
    let mut def = vec![None; ssa.values.len()];
    for blk in &ssa.blocks {
        for inst in &blk.insts {
            if let Some(d) = inst.dest {
                def[d as usize] = Some(&inst.op);
            }
        }
    }
    def
}

fn add_range(a: (i128, i128), b: (i128, i128)) -> Option<(i128, i128)> {
    Some((a.0.checked_add(b.0)?, a.1.checked_add(b.1)?))
}

fn sub_range(a: (i128, i128), b: (i128, i128)) -> Option<(i128, i128)> {
    Some((a.0.checked_sub(b.1)?, a.1.checked_sub(b.0)?))
}

fn mul_range(a: (i128, i128), b: (i128, i128)) -> Option<(i128, i128)> {
    let ps = [
        a.0.checked_mul(b.0)?,
        a.0.checked_mul(b.1)?,
        a.1.checked_mul(b.0)?,
        a.1.checked_mul(b.1)?,
    ];
    Some((*ps.iter().min().unwrap(), *ps.iter().max().unwrap()))
}

fn as_const(ranges: &FxHashMap<u32, (i128, i128)>, v: u32) -> Option<i128> {
    match ranges.get(&v) {
        Some((l, h)) if l == h => Some(*l),
        _ => None,
    }
}

fn is_one(def: &[Option<&SsaOp>], ranges: &FxHashMap<u32, (i128, i128)>, v: u32) -> bool {
    if matches!(def[v as usize], Some(SsaOp::ConstInt(1))) {
        return true;
    }
    as_const(ranges, v) == Some(1)
}

pub(super) fn no_overflow_steps(
    ssa: &SsaProto,
    preds: &[Vec<usize>],
    rpo_pos: &[usize],
    reached: &[bool],
) -> FxHashSet<u32> {
    let def = def_map(ssa);
    let pred_args: Vec<Vec<(u32, Vec<u32>)>> = (0..ssa.blocks.len())
        .map(|from| match &ssa.blocks[from].term {
            SsaTerm::Jump { target, args } => vec![(*target, args.clone())],
            SsaTerm::Branch {
                then_blk,
                then_args,
                else_blk,
                else_args,
                ..
            } => vec![
                (*then_blk, then_args.clone()),
                (*else_blk, else_args.clone()),
            ],
            SsaTerm::Return(_) | SsaTerm::Throw(_) | SsaTerm::Unreachable => vec![],
        })
        .collect();

    let mut headers: Vec<usize> = Vec::new();
    for header in 0..ssa.blocks.len() {
        if !reached[header] {
            continue;
        }
        if super::cfg::loop_body(preds, rpo_pos, reached, header).is_empty() {
            continue;
        }
        let blk = &ssa.blocks[header];
        let SsaTerm::Branch {
            cond,
            then_blk,
            else_blk,
            ..
        } = &blk.term
        else {
            continue;
        };
        let body = super::cfg::loop_body(preds, rpo_pos, reached, header);
        if !body.contains(&(*then_blk as usize)) || body.contains(&(*else_blk as usize)) {
            continue;
        }
        if !blk.insts.iter().any(|i| i.dest == Some(*cond)) {
            continue;
        }
        if !matches!(
            def[*cond as usize],
            Some(SsaOp::Binary {
                op: SsaBinOp::IntLt | SsaBinOp::IntLe | SsaBinOp::IntGt | SsaBinOp::IntGe,
                ..
            })
        ) {
            continue;
        }
        headers.push(header);
    }

    let bodies: FxHashMap<usize, rustc_hash::FxHashSet<usize>> = headers
        .iter()
        .map(|&h| (h, super::cfg::loop_body(preds, rpo_pos, reached, h)))
        .collect();

    let mut ranges: FxHashMap<u32, (i128, i128)> = FxHashMap::default();
    for (v, op) in def.iter().enumerate() {
        if let Some(SsaOp::ConstInt(c)) = op {
            if ssa.value_ty(v as u32) == SlotKind::Int {
                ranges.insert(v as u32, (*c as i128, *c as i128));
            }
        }
    }

    let mut converged = false;
    for _ in 0..64 {
        let mut changed = false;
        for (b, blk) in ssa.blocks.iter().enumerate() {
            if !reached[b] {
                continue;
            }
            for (pi, &p) in blk.params.iter().enumerate() {
                if ssa.value_ty(p) != SlotKind::Int {
                    continue;
                }
                let body = bodies.get(&b);
                let mut elo: Option<i128> = None;
                let mut ehi: Option<i128> = None;
                let mut ok = true;
                for pred in preds[b].iter().filter(|p| reached[**p]) {
                    let in_body = body.map_or(false, |bd| bd.contains(pred));
                    if in_body {
                        continue;
                    }
                    for (t, args) in pred_args[*pred].iter() {
                        if *t as usize != b {
                            continue;
                        }
                        let Some(&a) = args.get(pi) else {
                            ok = false;
                            continue;
                        };
                        match ranges.get(&a) {
                            Some(&(l, h)) => {
                                elo = Some(elo.map_or(l, |x: i128| x.min(l)));
                                ehi = Some(ehi.map_or(h, |x: i128| x.max(h)));
                            }
                            None => {
                                ok = false;
                            }
                        }
                    }
                }
                if !ok || elo.is_none() {
                    continue;
                }
                let mut l = elo.unwrap();
                let mut h = ehi.unwrap();
                let entry = (l, h);
                for pred in preds[b].iter().filter(|p| reached[**p]) {
                    let in_body = body.map_or(false, |bd| bd.contains(pred));
                    if !in_body {
                        continue;
                    }
                    for (t, args) in pred_args[*pred].iter() {
                        if *t as usize != b {
                            continue;
                        }
                        let Some(&a) = args.get(pi) else {
                            ok = false;
                            continue;
                        };
                        if a == p {
                            continue;
                        }
                        if let Some((cl, ch)) = step_contrib(ssa, &def, &ranges, b, p, a, entry) {
                            l = l.min(cl);
                            h = h.max(ch);
                            continue;
                        }
                        match ranges.get(&a) {
                            Some(&(al, ah)) => {
                                l = l.min(al);
                                h = h.max(ah);
                            }
                            None => {
                                ok = false;
                            }
                        }
                    }
                }
                if !ok {
                    continue;
                }
                if l > h {
                    continue;
                }
                if ranges.get(&p) != Some(&(l, h)) {
                    ranges.insert(p, (l, h));
                    changed = true;
                }
            }
            for inst in &blk.insts {
                let (Some(d), SsaOp::Binary { op, lhs, rhs }) = (inst.dest, &inst.op) else {
                    continue;
                };
                if ssa.value_ty(d) != SlotKind::Int {
                    continue;
                }
                let (Some(&a), Some(&c)) = (ranges.get(lhs), ranges.get(rhs)) else {
                    continue;
                };
                let r = match op {
                    SsaBinOp::IntAdd => add_range(a, c),
                    SsaBinOp::IntSub => sub_range(a, c),
                    SsaBinOp::IntMul => mul_range(a, c),
                    _ => None,
                };
                if let Some(r) = r {
                    if ranges.get(&d) != Some(&r) {
                        ranges.insert(d, r);
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            converged = true;
            break;
        }
    }
    if !converged {
        return FxHashSet::default();
    }

    let out: FxHashSet<u32> = ranges
        .iter()
        .filter(|&(_, &(l, h))| l >= I64_LO && h <= I64_HI)
        .filter(|(v, _)| {
            matches!(def[**v as usize], Some(SsaOp::Binary { op, .. }) if matches!(op, SsaBinOp::IntAdd | SsaBinOp::IntSub | SsaBinOp::IntMul))
        })
        .map(|(v, _)| *v)
        .collect();
    if super::super::trace() {
        eprintln!(
            "clif: ranges headers={} known={} no_overflow={}",
            headers.len(),
            ranges.len(),
            out.len()
        );
    }
    out
}

fn step_contrib(
    ssa: &SsaProto,
    def: &[Option<&SsaOp>],
    ranges: &FxHashMap<u32, (i128, i128)>,
    header: usize,
    param: u32,
    arg: u32,
    entry: (i128, i128),
) -> Option<(i128, i128)> {
    let blk = &ssa.blocks[header];
    let SsaTerm::Branch { cond, .. } = &blk.term else {
        return None;
    };
    let Some(SsaOp::Binary { op, lhs, rhs }) = def[*cond as usize] else {
        return None;
    };
    let le = matches!(op, SsaBinOp::IntLe | SsaBinOp::IntGe);
    let (k, bound, below) = match op {
        SsaBinOp::IntLt | SsaBinOp::IntLe => {
            if *lhs == param && blk.params.contains(&param) {
                (param, as_const(ranges, *rhs)?, true)
            } else if *rhs == param && blk.params.contains(&param) {
                (param, as_const(ranges, *lhs)?, false)
            } else {
                return None;
            }
        }
        SsaBinOp::IntGt | SsaBinOp::IntGe => {
            if *lhs == param && blk.params.contains(&param) {
                (param, as_const(ranges, *rhs)?, false)
            } else if *rhs == param && blk.params.contains(&param) {
                (param, as_const(ranges, *lhs)?, true)
            } else {
                return None;
            }
        }
        _ => return None,
    };
    let _ = k;
    let (elo, ehi) = entry;
    let Some(SsaOp::Binary {
        op: step,
        lhs: sl,
        rhs: sr,
    }) = def[arg as usize]
    else {
        return None;
    };
    let is_inc = matches!(step, SsaBinOp::IntAdd)
        && ((*sl == param && is_one(def, ranges, *sr))
            || (*sr == param && is_one(def, ranges, *sl)));
    let is_dec = matches!(step, SsaBinOp::IntSub) && *sl == param && is_one(def, ranges, *sr);
    if below && is_inc {
        let top = if le { bound + 1 } else { bound };
        Some((elo + 1, top))
    } else if !below && is_dec {
        let floor = if le { bound - 1 } else { bound };
        Some((floor, ehi - 1))
    } else {
        None
    }
}
