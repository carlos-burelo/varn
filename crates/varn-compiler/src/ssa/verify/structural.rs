use super::dominance::{check_use, dominators, DefSite};
use super::type_rules::check_inst_types;
use crate::ssa::ir::{Block, BlockId, InstKind, SsaFunc, Terminator, Value};

pub type VerifyResult = Result<(), String>;

pub fn verify(func: &SsaFunc) -> VerifyResult {
    let n = func.blocks.len();
    let mut def: Vec<Option<DefSite>> = vec![None; func.values.len()];

    let define = |v: Value, site: DefSite, def: &mut Vec<Option<DefSite>>| -> VerifyResult {
        let slot = def
            .get_mut(v.0 as usize)
            .ok_or_else(|| format!("value {} out of range", v.0))?;
        if slot.is_some() {
            return Err(format!("value v{} defined more than once", v.0));
        }
        *slot = Some(site);
        Ok(())
    };
    for (b, block) in func.blocks.iter().enumerate() {
        let bid = BlockId(b as u32);
        for p in &block.params {
            define(
                *p,
                DefSite {
                    block: bid,
                    order: 0,
                },
                &mut def,
            )?;
        }
        for (i, inst) in block.insts.iter().enumerate() {
            if let Some(d) = inst.dest {
                define(
                    d,
                    DefSite {
                        block: bid,
                        order: i as u32 + 1,
                    },
                    &mut def,
                )?;
            }
        }
    }

    for (b, block) in func.blocks.iter().enumerate() {
        for s in block_succs(block) {
            if s.0 as usize >= n {
                return Err(format!("block b{b} jumps to out-of-range block b{}", s.0));
            }
        }
        let nparams = block.params.len();
        for &pred in &block.preds {
            if pred.0 as usize >= n {
                return Err(format!("block b{b} has out-of-range pred b{}", pred.0));
            }
            let got = edge_arg_count(func, pred, BlockId(b as u32))?;
            if got != nparams {
                return Err(format!(
                    "edge b{}->b{b} carries {got} args but block has {nparams} params",
                    pred.0
                ));
            }
        }
    }

    let idom = dominators(func);
    for (b, block) in func.blocks.iter().enumerate() {
        let bid = BlockId(b as u32);

        for (i, inst) in block.insts.iter().enumerate() {
            let use_order = i as u32 + 1;
            check_inst_types(func, inst)?;
            for u in inst_uses(&inst.kind) {
                check_use(func, &def, &idom, u, bid, use_order)?;
            }
        }

        let end = block.insts.len() as u32 + 1;
        for u in term_value_uses(&block.term) {
            check_use(func, &def, &idom, u, bid, end)?;
        }

        for (target, args) in out_edges(&block.term) {
            for &a in args {
                check_use(func, &def, &idom, a, bid, u32::MAX)?;
            }
            let _ = target;
        }
    }
    Ok(())
}

pub(crate) fn recompute_preds(func: &mut SsaFunc) {
    let n = func.blocks.len();
    let mut preds = vec![Vec::new(); n];
    for (b_idx, block) in func.blocks.iter().enumerate() {
        let bid = BlockId(b_idx as u32);
        for succ in block_succs(block) {
            let slot = &mut preds[succ.0 as usize];
            if !slot.contains(&bid) {
                slot.push(bid);
            }
        }
    }
    for (b_idx, block) in func.blocks.iter_mut().enumerate() {
        block.preds = std::mem::take(&mut preds[b_idx]);
    }
}

pub(super) fn block_succs(block: &Block) -> Vec<BlockId> {
    let mut s = match &block.term {
        Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => Vec::new(),
        Terminator::Jump { target, .. } => vec![*target],
        Terminator::Branch {
            then_blk, else_blk, ..
        } => vec![*then_blk, *else_blk],
    };
    for inst in &block.insts {
        if let InstKind::Try { handler } = &inst.kind {
            s.push(*handler);
        }
    }
    s
}

fn edge_arg_count(func: &SsaFunc, pred: BlockId, block: BlockId) -> Result<usize, String> {
    for inst in &func.blocks[pred.0 as usize].insts {
        if let InstKind::Try { handler } = &inst.kind {
            if *handler == block {
                return Ok(0);
            }
        }
    }
    match &func.blocks[pred.0 as usize].term {
        Terminator::Jump { target, args } if *target == block => Ok(args.len()),
        Terminator::Branch {
            then_blk,
            then_args,
            else_blk,
            else_args,
            ..
        } => {
            if *then_blk == block {
                Ok(then_args.len())
            } else if *else_blk == block {
                Ok(else_args.len())
            } else {
                Err(format!("pred b{} has no edge to b{}", pred.0, block.0))
            }
        }
        Terminator::Return(_)
        | Terminator::Throw(_)
        | Terminator::Jump { .. }
        | Terminator::Unreachable => Err(format!("pred b{} has no edge to b{}", pred.0, block.0)),
    }
}

pub(crate) fn inst_uses(kind: &InstKind) -> Vec<Value> {
    let mut out = Vec::new();
    crate::ssa::uses::visit_uses(kind, &mut |v| out.push(v));
    out
}

pub(crate) fn term_value_uses(t: &Terminator) -> Vec<Value> {
    match t {
        Terminator::Return(Some(v)) => vec![*v],
        Terminator::Throw(v) => vec![*v],
        Terminator::Branch { cond, .. } => vec![*cond],
        Terminator::Return(_) | Terminator::Jump { .. } | Terminator::Unreachable => Vec::new(),
    }
}

pub(crate) fn out_edges(t: &Terminator) -> Vec<(BlockId, &Vec<Value>)> {
    match t {
        Terminator::Jump { target, args } => vec![(*target, args)],
        Terminator::Branch {
            then_blk,
            then_args,
            else_blk,
            else_args,
            ..
        } => {
            vec![(*then_blk, then_args), (*else_blk, else_args)]
        }
        Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => Vec::new(),
    }
}
