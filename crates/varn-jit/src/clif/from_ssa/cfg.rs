use rustc_hash::FxHashSet;
use varn_types::ssa::{SsaProto, SsaTerm};

use super::store::clif_ty;
use super::views;

pub(super) fn predecessors(ssa: &SsaProto) -> Vec<Vec<usize>> {
    let mut preds = vec![Vec::new(); ssa.blocks.len()];
    for (b, blk) in ssa.blocks.iter().enumerate() {
        let succs: &[u32] = match &blk.term {
            SsaTerm::Jump { target, .. } => std::slice::from_ref(target),
            SsaTerm::Branch {
                then_blk, else_blk, ..
            } => &[*then_blk, *else_blk],
            SsaTerm::Return(_) | SsaTerm::Throw(_) | SsaTerm::Unreachable => &[],
        };
        for s in succs {
            preds[*s as usize].push(b);
        }
        for inst in &blk.insts {
            if let varn_types::ssa::SsaOp::Try { catch_ip, .. } = &inst.op {
                if let Some(e) = ssa.catch_at(*catch_ip as usize) {
                    let h = e.block as usize;
                    if h < preds.len() && !preds[h].contains(&b) {
                        preds[h].push(b);
                    }
                }
            }
        }
    }
    preds
}

pub(super) fn loop_may_collect(
    ssa: &SsaProto,
    preds: &[Vec<usize>],
    header: usize,
    latch: usize,
) -> bool {
    let mut seen = vec![false; ssa.blocks.len()];
    seen[header] = true;
    let mut stack = vec![latch];
    let mut body = vec![header];
    while let Some(b) = stack.pop() {
        if std::mem::replace(&mut seen[b], true) {
            continue;
        }
        body.push(b);
        stack.extend(preds[b].iter().copied());
    }
    body.iter().any(|&b| {
        ssa.blocks[b]
            .insts
            .iter()
            .any(|i| !views::keeps_views(ssa, &i.op))
    })
}

pub(super) fn order(ssa: &SsaProto, entry: usize) -> Vec<usize> {
    let n = ssa.blocks.len();
    let mut visited = vec![false; n];
    let mut post = Vec::with_capacity(n);
    let mut stack: Vec<(usize, u8)> = vec![(entry, 0)];
    visited[entry] = true;
    while let Some(&(b, stage)) = stack.last() {
        stack.last_mut().unwrap().1 += 1;
        let mut succs: Vec<usize> = match &ssa.blocks[b].term {
            SsaTerm::Jump { target, .. } => vec![*target as usize],
            SsaTerm::Branch {
                then_blk, else_blk, ..
            } => vec![*else_blk as usize, *then_blk as usize],
            SsaTerm::Return(_) | SsaTerm::Throw(_) | SsaTerm::Unreachable => Vec::new(),
        };
        for inst in &ssa.blocks[b].insts {
            if let varn_types::ssa::SsaOp::Try { catch_ip, .. } = &inst.op {
                if let Some(e) = ssa.catch_at(*catch_ip as usize) {
                    let h = e.block as usize;
                    if h < n && !succs.contains(&h) {
                        succs.insert(0, h);
                    }
                }
            }
        }
        match succs.get(stage as usize) {
            Some(&s) => {
                if !visited[s] {
                    visited[s] = true;
                    stack.push((s, 0));
                }
            }
            None => {
                post.push(b);
                stack.pop();
            }
        }
    }
    post.into_iter().rev().collect()
}

pub(super) fn check_block_args(ssa: &SsaProto) -> Result<(), String> {
    let check = |from: usize, target: u32, args: &[u32]| -> Result<(), String> {
        let params = &ssa.blocks[target as usize].params;
        if params.len() != args.len() {
            return Err(format!(
                "from_ssa: block {from} passes {} values to block {target}, which takes {}",
                args.len(),
                params.len()
            ));
        }
        for (&a, &p) in args.iter().zip(params) {
            let (ak, pk) = (ssa.value_ty(a), ssa.value_ty(p));
            if clif_ty(ak) != clif_ty(pk) {
                return Err(format!(
                    "from_ssa: block {from} passes {ak:?} value {a} to {pk:?} parameter {p} of block {target}"
                ));
            }
        }
        Ok(())
    };
    for (b, blk) in ssa.blocks.iter().enumerate() {
        match &blk.term {
            SsaTerm::Jump { target, args } => check(b, *target, args)?,
            SsaTerm::Branch {
                then_blk,
                then_args,
                else_blk,
                else_args,
                ..
            } => {
                check(b, *then_blk, then_args)?;
                check(b, *else_blk, else_args)?;
            }
            SsaTerm::Return(_) | SsaTerm::Throw(_) | SsaTerm::Unreachable => {}
        }
    }
    Ok(())
}

pub(super) fn loop_body(
    preds: &[Vec<usize>],
    rpo_pos: &[usize],
    reached: &[bool],
    header: usize,
) -> FxHashSet<usize> {
    let live = |p: &usize| reached[*p];
    let latches: Vec<usize> = preds[header]
        .iter()
        .filter(|p| live(p))
        .copied()
        .filter(|&p| rpo_pos[header] <= rpo_pos[p])
        .collect();
    let mut body = FxHashSet::default();
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

pub(super) fn def_blocks(ssa: &SsaProto) -> Vec<Option<usize>> {
    let mut def = vec![None; ssa.values.len()];
    for (b, blk) in ssa.blocks.iter().enumerate() {
        for &p in &blk.params {
            def[p as usize] = Some(b);
        }
        for d in blk.insts.iter().filter_map(|i| i.dest) {
            def[d as usize] = Some(b);
        }
    }
    def
}

fn normal_succs(ssa: &SsaProto, b: usize) -> Vec<usize> {
    match &ssa.blocks[b].term {
        SsaTerm::Jump { target, .. } => vec![*target as usize],
        SsaTerm::Branch {
            then_blk, else_blk, ..
        } => vec![*then_blk as usize, *else_blk as usize],
        SsaTerm::Return(_) | SsaTerm::Throw(_) | SsaTerm::Unreachable => Vec::new(),
    }
}

fn block_exit(ssa: &SsaProto, entry: &[usize], b: usize) -> Option<Vec<usize>> {
    let mut stack = entry.to_vec();
    for inst in &ssa.blocks[b].insts {
        match &inst.op {
            varn_types::ssa::SsaOp::Try { catch_ip, .. } => {
                let e = ssa.catch_at(*catch_ip as usize)?;
                stack.push(e.block as usize);
            }
            varn_types::ssa::SsaOp::PopTry => {
                stack.pop()?;
            }
            _ => {}
        }
    }
    Some(stack)
}

fn stack_before_try(
    ssa: &SsaProto,
    entry: &[usize],
    b: usize,
    try_idx: usize,
) -> Option<Vec<usize>> {
    let mut stack = entry.to_vec();
    for (i, inst) in ssa.blocks[b].insts.iter().enumerate() {
        if i == try_idx {
            return Some(stack);
        }
        match &inst.op {
            varn_types::ssa::SsaOp::Try { catch_ip, .. } => {
                let e = ssa.catch_at(*catch_ip as usize)?;
                stack.push(e.block as usize);
            }
            varn_types::ssa::SsaOp::PopTry => {
                stack.pop()?;
            }
            _ => {}
        }
    }
    None
}

pub(super) fn try_entry_stacks(
    ssa: &SsaProto,
    rpo: &[usize],
    reached: &[bool],
) -> Option<Vec<Vec<usize>>> {
    let n = ssa.blocks.len();
    let mut entry: Vec<Option<Vec<usize>>> = vec![None; n];
    entry[ssa.entry as usize] = Some(Vec::new());
    for _ in 0..n + 2 {
        let mut changed = false;
        for &b in rpo {
            if !reached[b] {
                continue;
            }
            let Some(cur) = entry[b].clone() else {
                continue;
            };
            for (ti, inst) in ssa.blocks[b].insts.iter().enumerate() {
                if let varn_types::ssa::SsaOp::Try { catch_ip, .. } = &inst.op {
                    let Some(e) = ssa.catch_at(*catch_ip as usize) else {
                        return None;
                    };
                    let h = e.block as usize;
                    if h >= n {
                        return None;
                    }
                    let Some(outer) = stack_before_try(ssa, &cur, b, ti) else {
                        return None;
                    };
                    match &entry[h] {
                        None => {
                            entry[h] = Some(outer);
                            changed = true;
                        }
                        Some(prev) if *prev != outer => return None,
                        _ => {}
                    }
                }
            }
            let Some(exit) = block_exit(ssa, &cur, b) else {
                return None;
            };
            for s in normal_succs(ssa, b) {
                if s >= n || !reached[s] {
                    continue;
                }
                match &entry[s] {
                    None => {
                        entry[s] = Some(exit.clone());
                        changed = true;
                    }
                    Some(prev) if *prev != exit => return None,
                    _ => {}
                }
            }
        }
        if !changed {
            let mut out = Vec::with_capacity(n);
            for (i, e) in entry.into_iter().enumerate() {
                if reached[i] {
                    out.push(e?);
                } else {
                    out.push(Vec::new());
                }
            }
            return Some(out);
        }
    }
    None
}
