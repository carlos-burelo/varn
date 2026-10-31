use super::ir::{Block, BlockId, InstKind, SsaFunc, Terminator};

const UNREACHED: u32 = u32::MAX;

pub(crate) struct DomTree {
    tin: Vec<u32>,
    tout: Vec<u32>,
}

pub(crate) fn block_successors(block: &Block) -> Vec<BlockId> {
    let mut succs = match &block.term {
        Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => Vec::new(),
        Terminator::Jump { target, .. } => vec![*target],
        Terminator::Branch {
            then_blk, else_blk, ..
        } => vec![*then_blk, *else_blk],
    };
    for inst in &block.insts {
        if let InstKind::Try { handler } = &inst.kind {
            succs.push(*handler);
        }
    }
    succs
}

pub(crate) fn reverse_postorder(func: &SsaFunc) -> Vec<BlockId> {
    let n = func.blocks.len();
    let succs: Vec<Vec<BlockId>> = func.blocks.iter().map(block_successors).collect();
    let mut visited = vec![false; n];
    let mut post: Vec<BlockId> = Vec::with_capacity(n);
    let mut stack: Vec<(BlockId, usize)> = vec![(func.entry, 0)];
    visited[func.entry.0 as usize] = true;
    while let Some((block, next)) = stack.pop() {
        let out = &succs[block.0 as usize];
        if next < out.len() {
            stack.push((block, next + 1));
            let succ = out[next];
            if !visited[succ.0 as usize] {
                visited[succ.0 as usize] = true;
                stack.push((succ, 0));
            }
        } else {
            post.push(block);
        }
    }
    post.reverse();
    post
}

fn intersect(
    idom: &[Option<BlockId>],
    rpo_index: &[usize],
    mut a: BlockId,
    mut b: BlockId,
) -> BlockId {
    while a != b {
        while rpo_index[a.0 as usize] > rpo_index[b.0 as usize] {
            a = idom[a.0 as usize].expect("a processed block has an immediate dominator");
        }
        while rpo_index[b.0 as usize] > rpo_index[a.0 as usize] {
            b = idom[b.0 as usize].expect("a processed block has an immediate dominator");
        }
    }
    a
}

impl DomTree {
    pub(crate) fn compute(func: &SsaFunc) -> Self {
        let n = func.blocks.len();
        let rpo = reverse_postorder(func);
        let mut rpo_index = vec![usize::MAX; n];
        for (i, &b) in rpo.iter().enumerate() {
            rpo_index[b.0 as usize] = i;
        }
        let mut idom: Vec<Option<BlockId>> = vec![None; n];
        idom[func.entry.0 as usize] = Some(func.entry);
        let mut changed = true;
        while changed {
            changed = false;
            for &b in &rpo {
                if b == func.entry {
                    continue;
                }
                let mut candidate: Option<BlockId> = None;
                for &p in &func.blocks[b.0 as usize].preds {
                    if idom[p.0 as usize].is_none() {
                        continue;
                    }
                    candidate = Some(match candidate {
                        None => p,
                        Some(current) => intersect(&idom, &rpo_index, p, current),
                    });
                }
                if candidate.is_some() && idom[b.0 as usize] != candidate {
                    idom[b.0 as usize] = candidate;
                    changed = true;
                }
            }
        }
        let mut children: Vec<Vec<u32>> = vec![Vec::new(); n];
        for (b, parent) in idom.iter().enumerate() {
            if let Some(parent) = parent
                && parent.0 as usize != b
            {
                children[parent.0 as usize].push(b as u32);
            }
        }
        let mut tin = vec![UNREACHED; n];
        let mut tout = vec![UNREACHED; n];
        let mut clock = 0u32;
        let mut stack: Vec<(u32, usize)> = vec![(func.entry.0, 0)];
        tin[func.entry.0 as usize] = clock;
        clock += 1;
        while let Some((block, next)) = stack.pop() {
            if let Some(&child) = children[block as usize].get(next) {
                stack.push((block, next + 1));
                tin[child as usize] = clock;
                clock += 1;
                stack.push((child, 0));
            } else {
                tout[block as usize] = clock;
                clock += 1;
            }
        }
        Self { tin, tout }
    }

    pub(crate) fn dominates(&self, a: usize, b: usize) -> bool {
        if self.tin[b] == UNREACHED {
            return true;
        }
        if self.tin[a] == UNREACHED {
            return false;
        }
        self.tin[a] <= self.tin[b] && self.tout[b] <= self.tout[a]
    }

    pub(crate) fn reachable(&self, block: usize) -> bool {
        self.tin[block] != UNREACHED
    }
}
