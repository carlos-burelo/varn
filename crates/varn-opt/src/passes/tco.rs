use crate::ssa::ir::{InstKind, SsaFunc, Terminator};

pub fn run(func: &mut SsaFunc) -> bool {
    let mut changed = false;
    let entry_id = func.entry;

    for b_idx in 0..func.blocks.len() {
        let (has_tail_call, call_args) = match &func.blocks[b_idx].term {
            Terminator::Return(Some(ret_val)) => {
                let mut found_call = None;
                if let Some(last_inst) = func.blocks[b_idx].insts.last() {
                    if let Some(dest) = last_inst.dest {
                        if dest == *ret_val {
                            if let InstKind::SelfCall { args } = &last_inst.kind {
                                found_call = Some(args.clone());
                            }
                        }
                    }
                }
                if let Some(args) = found_call {
                    (true, args)
                } else {
                    (false, Vec::new())
                }
            }
            _ => (false, Vec::new()),
        };

        if has_tail_call {
            func.blocks[b_idx].insts.pop();
            func.blocks[b_idx].term = Terminator::Jump {
                target: entry_id,
                args: call_args,
            };
            if !func.blocks[entry_id.0 as usize].preds.contains(&crate::ssa::ir::BlockId(b_idx as u32)) {
                func.blocks[entry_id.0 as usize].preds.push(crate::ssa::ir::BlockId(b_idx as u32));
            }
            changed = true;
        }
    }

    changed
}
