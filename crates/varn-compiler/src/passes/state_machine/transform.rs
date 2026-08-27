//! CFG partitioning and state machine transformation for all suspendible functions.
//!
//! Transforms suspendible functions (`async function`, `function*`, `async function*`)
//! containing suspension points (`InstKind::Await`, `InstKind::Yield`) across linear
//! control flow, loops, and try/catch blocks.
//!
//! Splits blocks at suspension sites, introduces continuation blocks, computes exact
//! predecessors, and reorders blocks in canonical RPO order so that linear scan register
//! allocation preserves live ranges.

use crate::ssa::ir::{Block, BlockId, InstKind, SsaFunc, Terminator, Value};
use crate::ssa::suspend::SuspendPoint;

use super::layout::StateLayout;

/// Transforms a suspendible function into partitioned states.
/// Returns the calculated `state_size` in words.
pub fn transform_suspend_func(func: &mut SsaFunc, points: &[SuspendPoint]) -> u16 {
    let layout = StateLayout::compute(points);

    // Split blocks for each suspension point.
    for (k, pt) in points.iter().enumerate() {
        split_at_suspend_point(func, k, pt);
    }

    // Recompute exact CFG predecessors for all blocks after splitting.
    compute_preds(func);

    // Reorder blocks in topological RPO order so that newly allocated continuation
    // blocks precede their successors in linear indexing. This guarantees that
    // `Liveness::analyze` and `assign_registers` linear scan correctly compute
    // live intervals and do not clobber registers across suspension points.
    reorder_blocks_rpo(func);

    layout.state_size
}

/// Splits the block containing suspension point `k` into two blocks:
/// the prefix ending in the suspend instruction jumping to continuation `C_k`,
/// and the continuation `C_k` containing the suffix instructions and original terminator.
fn split_at_suspend_point(
    func: &mut SsaFunc,
    _k: usize,
    pt: &SuspendPoint,
) {
    // 1. Locate the block and instruction containing this suspension point.
    let (target_bid, inst_idx) = find_suspend_inst(func, pt.operand)
        .expect("suspend point instruction must exist in func blocks");

    // 2. Allocate the continuation block C_k.
    let cont_bid = func.alloc_block();

    // 3. Move suffix instructions [inst_idx + 1..] and the terminator from target_bid to cont_bid.
    let (suffix_insts, original_term) = {
        let block = func.block_mut(target_bid);
        let suffix = block.insts.split_off(inst_idx + 1);
        let term = std::mem::replace(&mut block.term, Terminator::Unreachable);
        (suffix, term)
    };

    // 4. Populate cont_bid with suffix instructions and original terminator.
    {
        let cont_block = func.block_mut(cont_bid);
        cont_block.insts = suffix_insts;
        cont_block.term = original_term;
    }

    // 5. Set target_bid's terminator to Jump { target: cont_bid, args: [] }.
    func.block_mut(target_bid).term = Terminator::Jump {
        target: cont_bid,
        args: Vec::new(),
    };
}

fn compute_preds(func: &mut SsaFunc) {
    let n = func.blocks.len();
    let mut preds = vec![Vec::new(); n];
    for (b_idx, block) in func.blocks.iter().enumerate() {
        let bid = BlockId(b_idx as u32);
        for succ in block_succs(block) {
            if !preds[succ.0 as usize].contains(&bid) {
                preds[succ.0 as usize].push(bid);
            }
        }
    }
    for (b_idx, block) in func.blocks.iter_mut().enumerate() {
        block.preds = std::mem::take(&mut preds[b_idx]);
    }
}

fn find_suspend_inst(func: &SsaFunc, operand: Value) -> Option<(BlockId, usize)> {
    for (b_idx, block) in func.blocks.iter().enumerate() {
        for (i_idx, inst) in block.insts.iter().enumerate() {
            match inst.kind {
                InstKind::Await { operand: op } | InstKind::Yield { operand: op } => {
                    if op == operand {
                        return Some((BlockId(b_idx as u32), i_idx));
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn get_term_succs(term: &Terminator) -> Vec<BlockId> {
    match term {
        Terminator::Jump { target, .. } => vec![*target],
        Terminator::Branch {
            then_blk, else_blk, ..
        } => vec![*then_blk, *else_blk],
        Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => Vec::new(),
    }
}

fn block_succs(block: &Block) -> Vec<BlockId> {
    let mut s = get_term_succs(&block.term);
    for inst in &block.insts {
        if let InstKind::Try { handler } = &inst.kind {
            s.push(*handler);
        }
    }
    s
}

fn reverse_postorder(func: &SsaFunc) -> Vec<BlockId> {
    let n = func.blocks.len();
    let mut visited = vec![false; n];
    let mut post: Vec<BlockId> = Vec::with_capacity(n);
    let mut stack: Vec<(BlockId, usize)> = Vec::with_capacity(n);
    let entry = func.entry;
    visited[entry.0 as usize] = true;
    stack.push((entry, 0));
    while let Some(top) = stack.last_mut() {
        let (b, idx) = *top;
        top.1 += 1;
        let s = block_succs(&func.blocks[b.0 as usize]);
        if idx < s.len() {
            let next = s[idx];
            if !visited[next.0 as usize] {
                visited[next.0 as usize] = true;
                stack.push((next, 0));
            }
        } else {
            post.push(b);
            stack.pop();
        }
    }
    post.reverse();
    post
}

fn reorder_blocks_rpo(func: &mut SsaFunc) {
    let order = reverse_postorder(func);
    let n = func.blocks.len();
    if order.len() == n && order.iter().enumerate().all(|(i, b)| b.0 as usize == i) {
        return;
    }

    let mut old_to_new = vec![BlockId(0); n];
    for (new_idx, &old_bid) in order.iter().enumerate() {
        old_to_new[old_bid.0 as usize] = BlockId(new_idx as u32);
    }
    let mut next_new = order.len() as u32;
    let mut full_order = order;
    for old_idx in 0..n {
        if !full_order.iter().any(|b| b.0 as usize == old_idx) {
            old_to_new[old_idx] = BlockId(next_new);
            next_new += 1;
            full_order.push(BlockId(old_idx as u32));
        }
    }

    func.entry = old_to_new[func.entry.0 as usize];
    for block in &mut func.blocks {
        for pred in &mut block.preds {
            *pred = old_to_new[pred.0 as usize];
        }
        for inst in &mut block.insts {
            if let InstKind::Try { handler } = &mut inst.kind {
                *handler = old_to_new[handler.0 as usize];
            }
        }
        match &mut block.term {
            Terminator::Jump { target, .. } => {
                *target = old_to_new[target.0 as usize];
            }
            Terminator::Branch { then_blk, else_blk, .. } => {
                *then_blk = old_to_new[then_blk.0 as usize];
                *else_blk = old_to_new[else_blk.0 as usize];
            }
            Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => {}
        }
    }

    let mut old_blocks = std::mem::take(&mut func.blocks);
    let mut new_blocks = Vec::with_capacity(n);
    for old_bid in full_order {
        new_blocks.push(std::mem::replace(
            &mut old_blocks[old_bid.0 as usize],
            Block {
                params: Vec::new(),
                insts: Vec::new(),
                term: Terminator::Unreachable,
                term_line: 0,
                preds: Vec::new(),
            },
        ));
    }
    func.blocks = new_blocks;
    compute_preds(func);
}
