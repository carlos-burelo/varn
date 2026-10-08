use super::cfg_dom::{dominates, dominators};
use crate::hir::{HirBinOp, HirType, HirUnOp};
use crate::ssa::ir::{BlockId, InstKind, SsaFunc, Terminator};

pub fn run(func: &mut SsaFunc) -> bool {
    let n = func.blocks.len();
    if n < 2 {
        return false;
    }

    let dom = dominators(func);
    let mut changed = false;

    let mut loops: rustc_hash::FxHashMap<BlockId, Vec<BlockId>> = rustc_hash::FxHashMap::default();
    for b in 0..n {
        for succ in successors(&func.blocks[b].term) {
            if dominates(&dom, succ.0 as usize, b) {
                loops.entry(succ).or_default().push(BlockId(b as u32));
            }
        }
    }

    let mut headers: Vec<BlockId> = loops.keys().copied().collect();
    headers.sort_by_key(|h| h.0);
    for header in headers {
        let latches = &loops[&header];
        changed |= hoist_loop(func, latches, header);
    }

    changed
}

fn hoist_loop(func: &mut SsaFunc, latches: &[BlockId], header: BlockId) -> bool {
    let n = func.blocks.len();

    let mut in_loop = vec![false; n];
    in_loop[header.0 as usize] = true;
    let mut stack = latches.to_vec();
    while let Some(b) = stack.pop() {
        if in_loop[b.0 as usize] {
            continue;
        }
        in_loop[b.0 as usize] = true;
        for &p in &func.blocks[b.0 as usize].preds {
            stack.push(p);
        }
    }

    let entries: Vec<BlockId> = func.blocks[header.0 as usize]
        .preds
        .iter()
        .copied()
        .filter(|p| !in_loop[p.0 as usize])
        .collect();
    let [pre] = entries.as_slice() else {
        return false;
    };
    let pre = *pre;
    if !matches!(
        func.blocks[pre.0 as usize].term,
        Terminator::Jump { target, .. } if target == header
    ) {
        return false;
    }

    let mut def_in_loop = vec![false; func.values.len()];
    let mut facts = LoopFacts {
        globals_stable: true,
        memory_stable: true,
    };
    for (b, block) in func.blocks.iter().enumerate() {
        if !in_loop[b] {
            continue;
        }
        for &p in &block.params {
            def_in_loop[p.0 as usize] = true;
        }
        for inst in &block.insts {
            if let Some(d) = inst.dest {
                def_in_loop[d.0 as usize] = true;
            }
            if !is_transparent(&inst.kind) {
                facts.globals_stable = false;
            }
            if !super::dce::is_pure(&inst.kind) {
                facts.memory_stable = false;
            }
        }
    }

    let mut changed = false;
    loop {
        let mut moved_any = false;
        for (b, &is_in) in in_loop.iter().enumerate().take(n) {
            if !is_in || b == pre.0 as usize {
                continue;
            }
            let mut i = 0;
            while i < func.blocks[b].insts.len() {
                let inst = &func.blocks[b].insts[i];
                let movable = hoistable(&inst.kind, facts)
                    && operands(&inst.kind)
                        .iter()
                        .all(|v| !def_in_loop[v.0 as usize]);
                if movable {
                    let inst = func.blocks[b].insts.remove(i);
                    if let Some(d) = inst.dest {
                        def_in_loop[d.0 as usize] = false;
                    }
                    func.blocks[pre.0 as usize].insts.push(inst);
                    moved_any = true;
                    changed = true;
                } else {
                    i += 1;
                }
            }
        }
        if !moved_any {
            break;
        }
    }
    changed
}

#[derive(Debug, Clone, Copy)]
struct LoopFacts {
    globals_stable: bool,

    memory_stable: bool,
}

fn hoistable(kind: &InstKind, facts: LoopFacts) -> bool {
    match kind {
        InstKind::GetFixedField { .. }
        | InstKind::ArrayGetIndex { .. }
        | InstKind::MapGetIndex { .. } => facts.memory_stable,

        InstKind::Binary { op, ty, .. } => {
            let typed = matches!(ty, HirType::Int | HirType::Float);
            let safe_op = matches!(
                op,
                HirBinOp::Add
                    | HirBinOp::Sub
                    | HirBinOp::Mul
                    | HirBinOp::Eq
                    | HirBinOp::Ne
                    | HirBinOp::Lt
                    | HirBinOp::Le
                    | HirBinOp::Gt
                    | HirBinOp::Ge
            );
            typed && safe_op
        }
        InstKind::Unary { op, ty, .. } => {
            matches!(ty, HirType::Int | HirType::Float | HirType::Bool)
                && matches!(op, HirUnOp::Neg | HirUnOp::Not)
        }
        InstKind::LoadGlobal(_) | InstKind::LoadGlobalIdx(_) | InstKind::LoadNativeGlobalIdx(_) => {
            facts.globals_stable
        }

        InstKind::IsNull { .. }
        | InstKind::IsArray { .. }
        | InstKind::GetEnumTag { .. }
        | InstKind::StrLength { .. } => true,
        InstKind::MakeClosure { upvalues_src, .. } => upvalues_src.is_empty(),
        InstKind::ConstInt(_)
        | InstKind::ConstFloat(_)
        | InstKind::ConstBool(_)
        | InstKind::ConstStr(_)
        | InstKind::ConstChar(_)
        | InstKind::ConstDecimal(_)
        | InstKind::ConstBigInt(_)
        | InstKind::ConstNull
        | InstKind::LoadUpvalue(_)
        | InstKind::StoreGlobal { .. }
        | InstKind::StoreGlobalIdx { .. }
        | InstKind::StoreUpvalue { .. }
        | InstKind::Call { .. }
        | InstKind::AllocInstance { .. }
        | InstKind::SelfCall { .. }
        | InstKind::GetProperty { .. }
        | InstKind::GetIndex { .. }
        | InstKind::SetProperty { .. }
        | InstKind::SetFixedField { .. }
        | InstKind::SetIndex { .. }
        | InstKind::ArraySetIndex { .. }
        | InstKind::MapSetIndex { .. }
        | InstKind::ArrayPush { .. }
        | InstKind::ObjectMerge { .. }
        | InstKind::MethodCall { .. }
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
        | InstKind::Await { .. }
        | InstKind::Spawn { .. }
        | InstKind::Yield { .. }
        | InstKind::IntrinsicCall { .. }
        | InstKind::CallNativeOp { .. }
        | InstKind::AssertNotNull { .. }
        | InstKind::GetPropertyMaybe { .. }
        | InstKind::ModuleSlot { .. }
        | InstKind::ArrayLength { .. }
        | InstKind::BytesLength { .. }
        | InstKind::This
        | InstKind::Range { .. }
        | InstKind::ObjectKeys { .. }
        | InstKind::GetSymbol { .. }
        | InstKind::IterCall { .. }
        | InstKind::GetSuper { .. }
        | InstKind::SuperCall { .. }
        | InstKind::SuperMethodCall { .. }
        | InstKind::ExtensionCall { .. }
        | InstKind::CallSpread { .. }
        | InstKind::BuildArraySpread { .. }
        | InstKind::BuildObjectSpread { .. } => false,
    }
}

fn is_transparent(kind: &InstKind) -> bool {
    matches!(
        kind,
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
            | InstKind::StoreUpvalue { .. }
            | InstKind::GetFixedField { .. }
            | InstKind::SetFixedField { .. }
            | InstKind::ArrayGetIndex { .. }
            | InstKind::ArraySetIndex { .. }
            | InstKind::MapGetIndex { .. }
            | InstKind::MapSetIndex { .. }
            | InstKind::IsNull { .. }
            | InstKind::IsArray { .. }
            | InstKind::StrLength { .. }
            | InstKind::ArrayLength { .. }
            | InstKind::BytesLength { .. }
            | InstKind::GetEnumTag { .. }
            | InstKind::MakeClosure { .. }
    )
}

fn operands(kind: &InstKind) -> Vec<crate::ssa::ir::Value> {
    crate::ssa::verify::inst_uses(kind)
}

fn successors(t: &Terminator) -> Vec<BlockId> {
    match t {
        Terminator::Jump { target, .. } => vec![*target],
        Terminator::Branch {
            then_blk, else_blk, ..
        } => vec![*then_blk, *else_blk],
        Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => Vec::new(),
    }
}
