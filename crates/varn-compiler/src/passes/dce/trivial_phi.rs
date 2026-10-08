use super::super::cfg::{dominates, dominators};
use crate::ssa::ir::{BlockId, SsaFunc, Terminator, Value};

pub(crate) fn eliminate_trivial_phis(func: &mut SsaFunc) -> bool {
    let n = func.blocks.len();
    if n < 2 {
        return false;
    }

    crate::ssa::verify::recompute_preds(func);

    let mut def_block = vec![None; func.values.len()];
    for (b_idx, block) in func.blocks.iter().enumerate() {
        let bid = BlockId(b_idx as u32);
        for &p in &block.params {
            def_block[p.0 as usize] = Some(bid);
        }
        for inst in &block.insts {
            if let Some(d) = inst.dest {
                def_block[d.0 as usize] = Some(bid);
            }
        }
    }

    let dom = dominators(func);
    let mut any_changed = false;

    loop {
        let mut changed = false;

        for b_idx in 0..func.blocks.len() {
            let b_id = BlockId(b_idx as u32);
            if b_id == func.entry {
                continue;
            }

            let mut pos = 0;
            while pos < func.blocks[b_id.0 as usize].params.len() {
                let phi = func.blocks[b_id.0 as usize].params[pos];

                let Some(incoming) = get_incoming_args(func, b_id, pos) else {
                    pos += 1;
                    continue;
                };

                if incoming.is_empty() {
                    pos += 1;
                    continue;
                }

                let mut same: Option<Value> = None;
                let mut is_trivial = true;

                for &arg in &incoming {
                    if arg == phi {
                        continue;
                    }
                    match same {
                        None => same = Some(arg),
                        Some(v) if v == arg => {}
                        _ => {
                            is_trivial = false;
                            break;
                        }
                    }
                }

                if !is_trivial {
                    pos += 1;
                    continue;
                }

                let Some(unique_val) = same else {
                    pos += 1;
                    continue;
                };

                if func.value_ty(unique_val) != func.value_ty(phi) {
                    pos += 1;
                    continue;
                }

                let Some(def_b) = def_block[unique_val.0 as usize] else {
                    pos += 1;
                    continue;
                };

                let can_replace = if def_b == b_id {
                    unique_val != phi && func.blocks[b_id.0 as usize].params.contains(&unique_val)
                } else {
                    dominates(&dom, def_b.0 as usize, b_id.0 as usize)
                };

                if !can_replace {
                    pos += 1;
                    continue;
                }

                func.replace_all_uses(phi, unique_val);
                remove_param(func, b_id, pos);
                changed = true;
                any_changed = true;
            }
        }

        if !changed {
            break;
        }
    }

    any_changed
}

fn get_incoming_args(func: &SsaFunc, block: BlockId, pos: usize) -> Option<Vec<Value>> {
    let mut args = Vec::new();
    let mut preds = func.blocks[block.0 as usize].preds.clone();
    preds.sort_unstable_by_key(|p| p.0);
    preds.dedup();
    for pred in preds {
        match &func.blocks[pred.0 as usize].term {
            Terminator::Jump {
                target,
                args: j_args,
            } => {
                if *target == block {
                    if let Some(&arg) = j_args.get(pos) {
                        args.push(arg);
                    } else {
                        return None;
                    }
                }
            }
            Terminator::Branch {
                then_blk,
                then_args,
                else_blk,
                else_args,
                ..
            } => {
                if *then_blk == block {
                    if let Some(&arg) = then_args.get(pos) {
                        args.push(arg);
                    } else {
                        return None;
                    }
                }
                if *else_blk == block {
                    if let Some(&arg) = else_args.get(pos) {
                        args.push(arg);
                    } else {
                        return None;
                    }
                }
            }
            Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => return None,
        }
    }
    Some(args)
}

pub(super) fn remove_param(func: &mut SsaFunc, block: BlockId, pos: usize) {
    func.blocks[block.0 as usize].params.remove(pos);
    let mut preds = func.blocks[block.0 as usize].preds.clone();
    preds.sort_unstable_by_key(|p| p.0);
    preds.dedup();
    for pred in preds {
        match &mut func.blocks[pred.0 as usize].term {
            Terminator::Jump { target, args } if *target == block => {
                if pos < args.len() {
                    args.remove(pos);
                }
            }
            Terminator::Branch {
                then_blk,
                then_args,
                else_blk,
                else_args,
                ..
            } => {
                if *then_blk == block && pos < then_args.len() {
                    then_args.remove(pos);
                }
                if *else_blk == block && pos < else_args.len() {
                    else_args.remove(pos);
                }
            }
            Terminator::Return(_)
            | Terminator::Throw(_)
            | Terminator::Jump { .. }
            | Terminator::Unreachable => {}
        }
    }
}
