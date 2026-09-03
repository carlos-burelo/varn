use super::ir::{Block, BlockId, Inst, InstKind, SsaFunc, Terminator, Value};
use crate::hir::HirType;

pub type VerifyResult = Result<(), String>;

#[derive(Clone, Copy)]
struct DefSite {
    block: BlockId,
    order: u32,
}

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

fn check_use(
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

fn block_succs(block: &Block) -> Vec<BlockId> {
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
        _ => Err(format!("pred b{} has no edge to b{}", pred.0, block.0)),
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
        _ => Vec::new(),
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
        _ => Vec::new(),
    }
}

fn dominators(func: &SsaFunc) -> Vec<Option<BlockId>> {
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

fn check_inst_types(func: &SsaFunc, inst: &Inst) -> VerifyResult {
    match &inst.kind {
        InstKind::Binary { op, lhs, rhs, ty } => {
            if *ty != HirType::Dynamic {
                let lty = func.value_ty(*lhs);
                let rty = func.value_ty(*rhs);
                if lty != *ty {
                    return Err(format!(
                        "typed binary {op:?}.{ty:?} expects lhs type {ty:?}, got v{}: {lty:?}",
                        lhs.0
                    ));
                }
                if rty != *ty {
                    return Err(format!(
                        "typed binary {op:?}.{ty:?} expects rhs type {ty:?}, got v{}: {rty:?}",
                        rhs.0
                    ));
                }
                if let Some(dest) = inst.dest {
                    let dty = func.value_ty(dest);
                    let expected_res = crate::hir::binary_result_ty(*op, *ty);
                    let is_str_concat = matches!(op, crate::hir::HirBinOp::Add)
                        && (lty == HirType::Str || rty == HirType::Str)
                        && dty == HirType::Str;
                    if dty != expected_res && !is_str_concat {
                        return Err(format!(
                            "typed binary {op:?}.{ty:?} defines v{}: {dty:?}, expected {expected_res:?}",
                            dest.0
                        ));
                    }
                }
            }
        }
        InstKind::Unary { op, operand, ty } => {
            if matches!(op, crate::hir::HirUnOp::Neg | crate::hir::HirUnOp::BitNot)
                && *ty != HirType::Dynamic
            {
                let oty = func.value_ty(*operand);
                if oty != *ty {
                    return Err(format!(
                        "typed unary {op:?}.{ty:?} expects operand type {ty:?}, got v{}: {oty:?}",
                        operand.0
                    ));
                }
            }
            if let Some(dest) = inst.dest {
                let dty = func.value_ty(dest);
                let expected = match op {
                    crate::hir::HirUnOp::Not => HirType::Bool,
                    crate::hir::HirUnOp::Typeof => HirType::Str,
                    crate::hir::HirUnOp::Neg | crate::hir::HirUnOp::BitNot => *ty,
                };
                if dty != expected && expected != HirType::Dynamic {
                    return Err(format!(
                        "typed unary {op:?}.{ty:?} defines v{}: {dty:?}, expected {expected:?}",
                        dest.0
                    ));
                }
            }
        }
        InstKind::ConstInt(_) => {
            if let Some(dest) = inst.dest {
                let dty = func.value_ty(dest);
                if dty != HirType::Int {
                    return Err(format!("ConstInt defines v{}: {dty:?}, expected Int", dest.0));
                }
            }
        }
        InstKind::ConstFloat(_) => {
            if let Some(dest) = inst.dest {
                let dty = func.value_ty(dest);
                if dty != HirType::Float {
                    return Err(format!("ConstFloat defines v{}: {dty:?}, expected Float", dest.0));
                }
            }
        }
        InstKind::ConstBool(_) => {
            if let Some(dest) = inst.dest {
                let dty = func.value_ty(dest);
                if dty != HirType::Bool {
                    return Err(format!("ConstBool defines v{}: {dty:?}, expected Bool", dest.0));
                }
            }
        }
        InstKind::ConstStr(_) => {
            if let Some(dest) = inst.dest {
                let dty = func.value_ty(dest);
                if dty != HirType::Str {
                    return Err(format!("ConstStr defines v{}: {dty:?}, expected Str", dest.0));
                }
            }
        }
        InstKind::ConstChar(_) => {
            if let Some(dest) = inst.dest {
                let dty = func.value_ty(dest);
                if dty != HirType::Int {
                    return Err(format!("ConstChar defines v{}: {dty:?}, expected Int", dest.0));
                }
            }
        }
        InstKind::IsNull { .. } => {
            if let Some(dest) = inst.dest {
                let dty = func.value_ty(dest);
                if dty != HirType::Bool {
                    return Err(format!("IsNull defines v{}: {dty:?}, expected Bool", dest.0));
                }
            }
        }
        InstKind::Cast { ty, .. } => {
            if let Some(dest) = inst.dest {
                let dty = func.value_ty(dest);
                if dty != *ty {
                    return Err(format!("Cast defines v{}: {dty:?}, expected {ty:?}", dest.0));
                }
            }
        }
        InstKind::ArrayGetIndex { index, .. } | InstKind::ArraySetIndex { index, .. } => {
            let ity = func.value_ty(*index);
            if ity != HirType::Int && ity != HirType::Dynamic {
                return Err(format!(
                    "Array index expects Int or Dynamic, got v{}: {ity:?}",
                    index.0
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

