use crate::ssa::ir::{Block, BlockId, InstKind, SsaFunc, Terminator, Value};
use crate::ssa::suspend::SuspendPoint;

use super::layout::StateLayout;

pub fn transform_suspend_func(func: &mut SsaFunc, points: &[SuspendPoint]) -> u16 {
    let layout = StateLayout::compute(points);

    for (k, pt) in points.iter().enumerate() {
        split_at_suspend_point(func, k, pt);
    }

    compute_preds(func);

    reorder_blocks_rpo(func);

    layout.state_size
}

fn split_at_suspend_point(func: &mut SsaFunc, _k: usize, pt: &SuspendPoint) {
    let (target_bid, inst_idx) = find_suspend_inst(func, pt.operand)
        .expect("suspend point instruction must exist in func blocks");

    let cont_bid = func.alloc_block();

    let (suffix_insts, original_term) = {
        let block = func.block_mut(target_bid);
        let suffix = block.insts.split_off(inst_idx + 1);
        let term = std::mem::replace(&mut block.term, Terminator::Unreachable);
        (suffix, term)
    };

    {
        let cont_block = func.block_mut(cont_bid);
        cont_block.insts = suffix_insts;
        cont_block.term = original_term;
    }

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
        for succ in crate::ssa::dom::block_successors(block) {
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
                InstKind::ConstInt(_)
                | InstKind::ConstFloat(_)
                | InstKind::ConstBool(_)
                | InstKind::ConstStr(_)
                | InstKind::ConstChar(_)
                | InstKind::ConstDecimal(_)
                | InstKind::ConstBigInt(_)
                | InstKind::ConstNull
                | InstKind::Binary { .. }
                | InstKind::Unary { .. }
                | InstKind::LoadGlobal(_)
                | InstKind::LoadGlobalIdx(_)
                | InstKind::LoadNativeGlobalIdx(_)
                | InstKind::LoadUpvalue(_)
                | InstKind::StoreGlobal { .. }
                | InstKind::StoreGlobalIdx { .. }
                | InstKind::StoreUpvalue { .. }
                | InstKind::Call { .. }
                | InstKind::AllocInstance { .. }
                | InstKind::SelfCall { .. }
                | InstKind::GetProperty { .. }
                | InstKind::GetFixedField { .. }
                | InstKind::GetIndex { .. }
                | InstKind::ArrayGetIndex { .. }
                | InstKind::BytesGetIndex { .. }
                | InstKind::MapGetIndex { .. }
                | InstKind::SetProperty { .. }
                | InstKind::SetFixedField { .. }
                | InstKind::SetIndex { .. }
                | InstKind::ArraySetIndex { .. }
                | InstKind::BytesSetIndex { .. }
                | InstKind::MapSetIndex { .. }
                | InstKind::ArrayPush { .. }
                | InstKind::ObjectMerge { .. }
                | InstKind::MethodCall { .. }
                | InstKind::IsNull { .. }
                | InstKind::Cast { .. }
                | InstKind::Convert { .. }
                | InstKind::BuildArray { .. }
                | InstKind::BuildTuple { .. }
                | InstKind::BuildObject { .. }
                | InstKind::BuildRecord { .. }
                | InstKind::BuildMap { .. }
                | InstKind::ObjectRest { .. }
                | InstKind::ToString { .. }
                | InstKind::BuildStr { .. }
                | InstKind::MakeClosure { .. }
                | InstKind::LoadCaptured { .. }
                | InstKind::StoreCaptured { .. }
                | InstKind::MakeClass { .. }
                | InstKind::DeclareLayout { .. }
                | InstKind::DefineStatic { .. }
                | InstKind::DefineMethod { .. }
                | InstKind::DefineAccessor { .. }
                | InstKind::MakeEnumVariant { .. }
                | InstKind::Try { .. }
                | InstKind::PopTry
                | InstKind::CatchParam { .. }
                | InstKind::CloseUpvalues { .. }
                | InstKind::Dispose { .. }
                | InstKind::LoadModule { .. }
                | InstKind::StoreModuleSlot { .. }
                | InstKind::Spawn { .. }
                | InstKind::IntrinsicCall { .. }
                | InstKind::CallNativeOp { .. }
                | InstKind::AssertNotNull { .. }
                | InstKind::GetPropertyMaybe { .. }
                | InstKind::ModuleSlot { .. }
                | InstKind::GetEnumTag { .. }
                | InstKind::IsArray { .. }
                | InstKind::StrLength { .. }
                | InstKind::ArrayLength { .. }
                | InstKind::BytesLength { .. }
                | InstKind::This
                | InstKind::Range { .. }
                | InstKind::ObjectKeys { .. }
                | InstKind::GetSuper { .. }
                | InstKind::SuperCall { .. }
                | InstKind::SuperMethodCall { .. }
                | InstKind::ExtensionCall { .. }
                | InstKind::CallSpread { .. }
                | InstKind::BuildArraySpread { .. }
                | InstKind::BuildObjectSpread { .. } => {}
            }
        }
    }
    None
}

fn reorder_blocks_rpo(func: &mut SsaFunc) {
    let order = crate::ssa::dom::reverse_postorder(func);
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
    for (old_idx, slot) in old_to_new.iter_mut().enumerate().take(n) {
        if !full_order.iter().any(|b| b.0 as usize == old_idx) {
            *slot = BlockId(next_new);
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
            Terminator::Branch {
                then_blk, else_blk, ..
            } => {
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
