use crate::ssa::ir::{BlockId, InstKind, SsaFunc, Terminator, Value};
use rustc_hash::FxHashSet;

pub fn run(func: &mut SsaFunc) -> bool {
    let mut changed = false;
    let mut used = FxHashSet::default();

    for block in &func.blocks {
        for inst in &block.insts {
            add_inst_uses(&inst.kind, &mut used);
        }
        add_term_uses(&block.term, &mut used);
    }

    for block_idx in 0..func.blocks.len() {
        let b_id = BlockId(block_idx as u32);
        let mut new_insts = Vec::new();

        let old_insts = std::mem::take(&mut func.blocks[b_id.0 as usize].insts);
        for inst in old_insts {
            if let Some(dest) = inst.dest {
                if !used.contains(&dest) && !has_side_effects(&inst.kind) {
                    changed = true;
                    continue;
                }
            }
            new_insts.push(inst);
        }
        func.blocks[b_id.0 as usize].insts = new_insts;
    }

    for b_idx in 0..func.blocks.len() {
        let b_id = BlockId(b_idx as u32);
        if b_id == func.entry {
            continue;
        }

        let mut pos = 0;
        while pos < func.blocks[b_id.0 as usize].params.len() {
            let phi = func.blocks[b_id.0 as usize].params[pos];
            if !used.contains(&phi) {
                remove_param(func, b_id, pos);
                changed = true;
            } else {
                pos += 1;
            }
        }
    }

    changed
}

fn has_side_effects(kind: &InstKind) -> bool {
    matches!(
        kind,
        InstKind::StoreGlobal { .. }
            | InstKind::StoreUpvalue { .. }
            | InstKind::Call { .. }
            | InstKind::SelfCall { .. }
            | InstKind::SetProperty { .. }
            | InstKind::SetIndex { .. }
            | InstKind::ArraySetIndex { .. }
            | InstKind::ObjectMerge { .. }
            | InstKind::MethodCall { .. }
            | InstKind::IntrinsicCall { .. }
            | InstKind::CallNativeOp { .. }
            | InstKind::AssertNotNull { .. }
            | InstKind::IterCall { .. }
            | InstKind::SuperCall { .. }
            | InstKind::SuperMethodCall { .. }
            | InstKind::ExtensionCall { .. }
            | InstKind::CallSpread { .. }
            | InstKind::StoreCaptured { .. }
            | InstKind::MakeClass { .. }
            | InstKind::DeclareField { .. }
            | InstKind::DefineStatic { .. }
            | InstKind::DefineMethod { .. }
            | InstKind::DefineAccessor { .. }
            | InstKind::Try { .. }
            | InstKind::PopTry
            | InstKind::CloseUpvalues { .. }
            | InstKind::Dispose { .. }
            | InstKind::LoadModule { .. }
            | InstKind::StoreModuleSlot { .. }
            | InstKind::Await { .. }
            | InstKind::Spawn { .. }
            | InstKind::Yield { .. }
    )
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

fn add_inst_uses(kind: &InstKind, used: &mut FxHashSet<Value>) {
    used.extend(crate::ssa::verify::inst_uses(kind));
}

fn add_term_uses(term: &Terminator, used: &mut FxHashSet<Value>) {
    match term {
        Terminator::Return(Some(v)) => {
            used.insert(*v);
        }
        Terminator::Throw(v) => {
            used.insert(*v);
        }
        Terminator::Jump { args, .. } => {
            for &arg in args {
                used.insert(arg);
            }
        }
        Terminator::Branch {
            cond,
            then_args,
            else_args,
            ..
        } => {
            used.insert(*cond);
            for &arg in then_args {
                used.insert(arg);
            }
            for &arg in else_args {
                used.insert(arg);
            }
        }
        _ => {}
    }
}
