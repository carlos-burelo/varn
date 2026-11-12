use crate::ssa::ir::{InstKind, SsaFunc, Terminator};

pub(super) fn emission_order(ssa: &SsaFunc) -> Vec<usize> {
    let n = ssa.blocks.len();

    let succs = |b: usize| -> Vec<usize> {
        let mut s = match &ssa.blocks[b].term {
            Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => Vec::new(),
            Terminator::Jump { target, .. } => vec![target.0 as usize],
            Terminator::Branch {
                then_blk, else_blk, ..
            } => {
                vec![else_blk.0 as usize, then_blk.0 as usize]
            }
        };
        for inst in &ssa.blocks[b].insts {
            if let InstKind::Try { handler } = &inst.kind {
                s.push(handler.0 as usize);
            }
        }
        s
    };
    let mut visited = vec![false; n];
    let mut post: Vec<usize> = Vec::with_capacity(n);
    let mut stack: Vec<(usize, u8)> = Vec::with_capacity(n);
    let entry = ssa.entry.0 as usize;
    visited[entry] = true;
    stack.push((entry, 0));
    while let Some(top) = stack.last_mut() {
        let (b, stage) = *top;
        top.1 += 1;
        let succ = succs(b).get(stage as usize).copied();
        match succ {
            Some(s) => {
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
    let mut order: Vec<usize> = post.into_iter().rev().collect();
    for (b, seen) in visited.iter().enumerate() {
        if !seen {
            order.push(b);
        }
    }
    order
}
