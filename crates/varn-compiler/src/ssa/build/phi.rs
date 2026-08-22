use super::super::ir::{BlockId, SsaFunc, Terminator, Value};

pub(super) fn simplify_phis(func: &mut SsaFunc) {
    loop {
        let mut removed = None;
        'scan: for b in 0..func.blocks.len() {
            if BlockId(b as u32) == func.entry {
                continue;
            }
            for pos in 0..func.blocks[b].params.len() {
                let phi = func.blocks[b].params[pos];
                if let Some(same) = trivial_operand(func, BlockId(b as u32), pos, phi) {
                    remove_param(func, BlockId(b as u32), pos);
                    removed = Some((phi, same));
                    break 'scan;
                }
            }
        }
        match removed {
            Some((phi, same)) => func.replace_all_uses(phi, same),
            None => break,
        }
    }
}

fn trivial_operand(func: &SsaFunc, block: BlockId, pos: usize, phi: Value) -> Option<Value> {
    let mut same: Option<Value> = None;
    for &pred in &func.blocks[block.0 as usize].preds {
        let op = edge_arg(func, pred, block, pos);
        if op == phi || same == Some(op) {
            continue;
        }
        if same.is_some() {
            return None;
        }
        same = Some(op);
    }
    Some(same.unwrap_or(phi))
}

fn edge_arg(func: &SsaFunc, pred: BlockId, block: BlockId, pos: usize) -> Value {
    match &func.blocks[pred.0 as usize].term {
        Terminator::Jump { target, args } if *target == block => args[pos],
        Terminator::Branch {
            then_blk,
            then_args,
            else_blk,
            else_args,
            ..
        } => {
            if *then_blk == block {
                then_args[pos]
            } else {
                debug_assert_eq!(*else_blk, block);
                else_args[pos]
            }
        }
        _ => panic!("no edge {pred:?} -> {block:?}"),
    }
}

fn remove_param(func: &mut SsaFunc, block: BlockId, pos: usize) {
    func.blocks[block.0 as usize].params.remove(pos);
    let preds = func.blocks[block.0 as usize].preds.clone();
    for pred in preds {
        match &mut func.blocks[pred.0 as usize].term {
            Terminator::Jump { target, args } if *target == block => {
                args.remove(pos);
            }
            Terminator::Branch {
                then_blk,
                then_args,
                else_blk,
                else_args,
                ..
            } => {
                if *then_blk == block {
                    then_args.remove(pos);
                }
                if *else_blk == block {
                    else_args.remove(pos);
                }
            }
            _ => {}
        }
    }
}
