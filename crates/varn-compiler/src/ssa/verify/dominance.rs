use super::structural::VerifyResult;
use crate::ssa::ir::{BlockId, SsaFunc, Value};

#[derive(Clone, Copy)]
pub(super) struct DefSite {
    pub(super) block: BlockId,
    pub(super) order: u32,
}

pub(super) fn check_use(
    func: &SsaFunc,
    def: &[Option<DefSite>],
    idom: &[Option<BlockId>],
    v: Value,
    use_block: BlockId,
    use_order: u32,
) -> VerifyResult {
    let site = def
        .get(v.0 as usize)
        .copied()
        .flatten()
        .ok_or_else(|| format!("use of undefined value v{}", v.0))?;
    if site.block == use_block {
        if site.order >= use_order {
            return Err(format!("value v{} used before its definition", v.0));
        }
        return Ok(());
    }
    if dominates(idom, func.entry, site.block, use_block) {
        Ok(())
    } else {
        Err(format!(
            "def of v{} (b{}) does not dominate use (b{})",
            v.0, site.block.0, use_block.0
        ))
    }
}

pub(super) fn dominators(func: &SsaFunc) -> Vec<Option<BlockId>> {
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
        for &b in rpo.iter() {
            if b == func.entry {
                continue;
            }
            let mut new_idom: Option<BlockId> = None;
            for &p in &func.blocks[b.0 as usize].preds {
                if idom[p.0 as usize].is_none() {
                    continue;
                }
                new_idom = Some(match new_idom {
                    None => p,
                    Some(cur) => intersect(&idom, &rpo_index, p, cur),
                });
            }
            if new_idom.is_some() && idom[b.0 as usize] != new_idom {
                idom[b.0 as usize] = new_idom;
                changed = true;
            }
        }
    }
    idom
}

fn intersect(
    idom: &[Option<BlockId>],
    rpo_index: &[usize],
    mut a: BlockId,
    mut b: BlockId,
) -> BlockId {
    while a != b {
        while rpo_index[a.0 as usize] > rpo_index[b.0 as usize] {
            a = idom[a.0 as usize].expect("processed block has idom");
        }
        while rpo_index[b.0 as usize] > rpo_index[a.0 as usize] {
            b = idom[b.0 as usize].expect("processed block has idom");
        }
    }
    a
}

fn dominates(idom: &[Option<BlockId>], entry: BlockId, a: BlockId, b: BlockId) -> bool {
    let mut cur = b;
    loop {
        if cur == a {
            return true;
        }
        if cur == entry {
            return a == entry;
        }
        match idom[cur.0 as usize] {
            Some(next) if next != cur => cur = next,
            _ => return false,
        }
    }
}

fn reverse_postorder(func: &SsaFunc) -> Vec<BlockId> {
    use super::structural::block_succs;
    let n = func.blocks.len();
    let mut visited = vec![false; n];
    let mut post: Vec<BlockId> = Vec::with_capacity(n);

    let mut stack: Vec<(BlockId, usize)> = vec![(func.entry, 0)];
    visited[func.entry.0 as usize] = true;
    while let Some((b, idx)) = stack.pop() {
        let s = block_succs(&func.blocks[b.0 as usize]);
        if idx < s.len() {
            stack.push((b, idx + 1));
            let next = s[idx];
            if !visited[next.0 as usize] {
                visited[next.0 as usize] = true;
                stack.push((next, 0));
            }
        } else {
            post.push(b);
        }
    }
    post.reverse();
    post
}
