//! Critical-edge splitting: gives every phi predecessor a block of its own so
//! the edge copies emitted for it cannot land in a block another edge shares.

use super::super::ir::{Block, BlockId, SsaFunc, Terminator};

pub(super) fn split_phi_edges(ssa: &mut SsaFunc) {
    let original = ssa.blocks.len();
    for b in 0..original {
        let (then_has, else_has) = match &ssa.blocks[b].term {
            Terminator::Branch {
                then_blk, else_blk, ..
            } => (
                !ssa.blocks[then_blk.0 as usize].params.is_empty(),
                !ssa.blocks[else_blk.0 as usize].params.is_empty(),
            ),
            _ => continue,
        };
        if then_has {
            split_one(ssa, BlockId(b as u32), true);
        }
        if else_has {
            split_one(ssa, BlockId(b as u32), false);
        }
    }
}

fn split_one(ssa: &mut SsaFunc, br: BlockId, is_then: bool) {
    let (target, args) = match &mut ssa.blocks[br.0 as usize].term {
        Terminator::Branch {
            then_blk,
            then_args,
            else_blk,
            else_args,
            ..
        } => {
            if is_then {
                (*then_blk, std::mem::take(then_args))
            } else {
                (*else_blk, std::mem::take(else_args))
            }
        }
        _ => return,
    };
    let pad = BlockId(ssa.blocks.len() as u32);
    ssa.blocks.push(Block {
        params: Vec::new(),
        insts: Vec::new(),
        term: Terminator::Jump { target, args },
        preds: vec![br],
    });
    if let Terminator::Branch {
        then_blk, else_blk, ..
    } = &mut ssa.blocks[br.0 as usize].term
    {
        if is_then {
            *then_blk = pad;
        } else {
            *else_blk = pad;
        }
    }
    for p in ssa.blocks[target.0 as usize].preds.iter_mut() {
        if *p == br {
            *p = pad;
            break;
        }
    }
}

