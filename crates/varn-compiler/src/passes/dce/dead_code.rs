use super::purity::{dest_droppable, is_pure};
use super::trivial_phi::{eliminate_trivial_phis, remove_param};
use crate::ssa::ir::{BlockId, InstKind, SsaFunc, Terminator, Value};
use rustc_hash::FxHashSet;

pub fn run(func: &mut SsaFunc) -> bool {
    let mut changed = false;

    changed |= eliminate_trivial_phis(func);

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
        for mut inst in old_insts {
            if let Some(dest) = inst.dest {
                if !used.contains(&dest) {
                    if is_pure(&inst.kind) {
                        changed = true;
                        continue;
                    }
                    if dest_droppable(&inst.kind) {
                        inst.dest = None;
                        changed = true;
                    }
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
        Terminator::Return(_) | Terminator::Unreachable => {}
    }
}
