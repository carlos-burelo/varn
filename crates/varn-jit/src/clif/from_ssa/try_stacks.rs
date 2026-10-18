use super::cfg::normal_succs;
use varn_types::ssa::SsaProto;

pub(super) fn block_exit(ssa: &SsaProto, entry: &[usize], b: usize) -> Option<Vec<usize>> {
    let mut stack = entry.to_vec();
    for inst in &ssa.blocks[b].insts {
        match &inst.op {
            varn_types::ssa::SsaOp::Try { catch_ip, .. } => {
                let e = ssa.catch_at(*catch_ip as usize)?;
                stack.push(e.block as usize);
            }
            varn_types::ssa::SsaOp::PopTry => {
                stack.pop()?;
            }
            varn_types::ssa::SsaOp::ConstInt(_)
            | varn_types::ssa::SsaOp::ConstFloat(_)
            | varn_types::ssa::SsaOp::ConstBool(_)
            | varn_types::ssa::SsaOp::ConstNull
            | varn_types::ssa::SsaOp::ConstStr(_)
            | varn_types::ssa::SsaOp::Binary { .. }
            | varn_types::ssa::SsaOp::Unary { .. }
            | varn_types::ssa::SsaOp::SelfCall { .. }
            | varn_types::ssa::SsaOp::Call { .. }
            | varn_types::ssa::SsaOp::AllocInstance { .. }
            | varn_types::ssa::SsaOp::LoadGlobalIdx(_)
            | varn_types::ssa::SsaOp::ArrayGetIndex { .. }
            | varn_types::ssa::SsaOp::BytesGetIndex { .. }
            | varn_types::ssa::SsaOp::ArraySetIndex { .. }
            | varn_types::ssa::SsaOp::BytesSetIndex { .. }
            | varn_types::ssa::SsaOp::ConstChar(_)
            | varn_types::ssa::SsaOp::ConstBigInt(_)
            | varn_types::ssa::SsaOp::ConstDecimal(_)
            | varn_types::ssa::SsaOp::MakeEnumVariant { .. }
            | varn_types::ssa::SsaOp::IntrinsicCall { .. }
            | varn_types::ssa::SsaOp::LoadNativeGlobalIdx(_)
            | varn_types::ssa::SsaOp::CatchParam { .. }
            | varn_types::ssa::SsaOp::StoreGlobalIdx { .. }
            | varn_types::ssa::SsaOp::MakeClosure { .. }
            | varn_types::ssa::SsaOp::LoadCaptured { .. }
            | varn_types::ssa::SsaOp::StoreCaptured { .. }
            | varn_types::ssa::SsaOp::LoadUpvalue(_)
            | varn_types::ssa::SsaOp::StoreUpvalue { .. }
            | varn_types::ssa::SsaOp::CloseUpvalues { .. }
            | varn_types::ssa::SsaOp::Cast { .. }
            | varn_types::ssa::SsaOp::Convert { .. }
            | varn_types::ssa::SsaOp::IsNull { .. }
            | varn_types::ssa::SsaOp::Typeof { .. }
            | varn_types::ssa::SsaOp::ToString { .. }
            | varn_types::ssa::SsaOp::IsArray { .. }
            | varn_types::ssa::SsaOp::GetEnumTag { .. }
            | varn_types::ssa::SsaOp::ObjectKeys { .. }
            | varn_types::ssa::SsaOp::BuildStr { .. }
            | varn_types::ssa::SsaOp::BuildArray { .. }
            | varn_types::ssa::SsaOp::BuildMap { .. }
            | varn_types::ssa::SsaOp::BuildObject { .. }
            | varn_types::ssa::SsaOp::GetProperty { .. }
            | varn_types::ssa::SsaOp::SetProperty { .. }
            | varn_types::ssa::SsaOp::GetIndex { .. }
            | varn_types::ssa::SsaOp::SetIndex { .. }
            | varn_types::ssa::SsaOp::ArrayLength { .. }
            | varn_types::ssa::SsaOp::StrLength { .. }
            | varn_types::ssa::SsaOp::BytesLength { .. }
            | varn_types::ssa::SsaOp::MethodCall { .. }
            | varn_types::ssa::SsaOp::CallNativeOp { .. }
            | varn_types::ssa::SsaOp::ArrayPush { .. }
            | varn_types::ssa::SsaOp::This
            | varn_types::ssa::SsaOp::GetFixedField { .. }
            | varn_types::ssa::SsaOp::SetFixedField { .. }
            | varn_types::ssa::SsaOp::MakeClass { .. }
            | varn_types::ssa::SsaOp::DeclareLayout { .. }
            | varn_types::ssa::SsaOp::DefineMethod { .. }
            | varn_types::ssa::SsaOp::GetSuper { .. }
            | varn_types::ssa::SsaOp::LoadGlobal(_)
            | varn_types::ssa::SsaOp::StoreGlobal { .. }
            | varn_types::ssa::SsaOp::BuildTuple { .. }
            | varn_types::ssa::SsaOp::BuildArraySpread { .. }
            | varn_types::ssa::SsaOp::BuildObjectSpread { .. }
            | varn_types::ssa::SsaOp::ObjectMerge { .. }
            | varn_types::ssa::SsaOp::ObjectRest { .. }
            | varn_types::ssa::SsaOp::GetPropertyMaybe { .. }
            | varn_types::ssa::SsaOp::AssertNotNull { .. }
            | varn_types::ssa::SsaOp::BindMethod { .. }
            | varn_types::ssa::SsaOp::ArrayExtend { .. }
            | varn_types::ssa::SsaOp::WrapSpread { .. }
            | varn_types::ssa::SsaOp::Range { .. }
            | varn_types::ssa::SsaOp::GetSymbol { .. }
            | varn_types::ssa::SsaOp::IterCall { .. }
            | varn_types::ssa::SsaOp::SuperCall { .. }
            | varn_types::ssa::SsaOp::SuperMethodCall { .. }
            | varn_types::ssa::SsaOp::ExtensionCall { .. }
            | varn_types::ssa::SsaOp::CallSpread { .. }
            | varn_types::ssa::SsaOp::LoadModule { .. }
            | varn_types::ssa::SsaOp::ModuleSlot { .. }
            | varn_types::ssa::SsaOp::StoreModuleSlot { .. }
            | varn_types::ssa::SsaOp::Await { .. }
            | varn_types::ssa::SsaOp::Spawn { .. }
            | varn_types::ssa::SsaOp::Yield { .. }
            | varn_types::ssa::SsaOp::Dispose { .. } => {}
        }
    }
    Some(stack)
}

pub(super) fn stack_before_try(
    ssa: &SsaProto,
    entry: &[usize],
    b: usize,
    try_idx: usize,
) -> Option<Vec<usize>> {
    let mut stack = entry.to_vec();
    for (i, inst) in ssa.blocks[b].insts.iter().enumerate() {
        if i == try_idx {
            return Some(stack);
        }
        match &inst.op {
            varn_types::ssa::SsaOp::Try { catch_ip, .. } => {
                let e = ssa.catch_at(*catch_ip as usize)?;
                stack.push(e.block as usize);
            }
            varn_types::ssa::SsaOp::PopTry => {
                stack.pop()?;
            }
            varn_types::ssa::SsaOp::ConstInt(_)
            | varn_types::ssa::SsaOp::ConstFloat(_)
            | varn_types::ssa::SsaOp::ConstBool(_)
            | varn_types::ssa::SsaOp::ConstNull
            | varn_types::ssa::SsaOp::ConstStr(_)
            | varn_types::ssa::SsaOp::Binary { .. }
            | varn_types::ssa::SsaOp::Unary { .. }
            | varn_types::ssa::SsaOp::SelfCall { .. }
            | varn_types::ssa::SsaOp::Call { .. }
            | varn_types::ssa::SsaOp::AllocInstance { .. }
            | varn_types::ssa::SsaOp::LoadGlobalIdx(_)
            | varn_types::ssa::SsaOp::ArrayGetIndex { .. }
            | varn_types::ssa::SsaOp::BytesGetIndex { .. }
            | varn_types::ssa::SsaOp::ArraySetIndex { .. }
            | varn_types::ssa::SsaOp::BytesSetIndex { .. }
            | varn_types::ssa::SsaOp::ConstChar(_)
            | varn_types::ssa::SsaOp::ConstBigInt(_)
            | varn_types::ssa::SsaOp::ConstDecimal(_)
            | varn_types::ssa::SsaOp::MakeEnumVariant { .. }
            | varn_types::ssa::SsaOp::IntrinsicCall { .. }
            | varn_types::ssa::SsaOp::LoadNativeGlobalIdx(_)
            | varn_types::ssa::SsaOp::CatchParam { .. }
            | varn_types::ssa::SsaOp::StoreGlobalIdx { .. }
            | varn_types::ssa::SsaOp::MakeClosure { .. }
            | varn_types::ssa::SsaOp::LoadCaptured { .. }
            | varn_types::ssa::SsaOp::StoreCaptured { .. }
            | varn_types::ssa::SsaOp::LoadUpvalue(_)
            | varn_types::ssa::SsaOp::StoreUpvalue { .. }
            | varn_types::ssa::SsaOp::CloseUpvalues { .. }
            | varn_types::ssa::SsaOp::Cast { .. }
            | varn_types::ssa::SsaOp::Convert { .. }
            | varn_types::ssa::SsaOp::IsNull { .. }
            | varn_types::ssa::SsaOp::Typeof { .. }
            | varn_types::ssa::SsaOp::ToString { .. }
            | varn_types::ssa::SsaOp::IsArray { .. }
            | varn_types::ssa::SsaOp::GetEnumTag { .. }
            | varn_types::ssa::SsaOp::ObjectKeys { .. }
            | varn_types::ssa::SsaOp::BuildStr { .. }
            | varn_types::ssa::SsaOp::BuildArray { .. }
            | varn_types::ssa::SsaOp::BuildMap { .. }
            | varn_types::ssa::SsaOp::BuildObject { .. }
            | varn_types::ssa::SsaOp::GetProperty { .. }
            | varn_types::ssa::SsaOp::SetProperty { .. }
            | varn_types::ssa::SsaOp::GetIndex { .. }
            | varn_types::ssa::SsaOp::SetIndex { .. }
            | varn_types::ssa::SsaOp::ArrayLength { .. }
            | varn_types::ssa::SsaOp::StrLength { .. }
            | varn_types::ssa::SsaOp::BytesLength { .. }
            | varn_types::ssa::SsaOp::MethodCall { .. }
            | varn_types::ssa::SsaOp::CallNativeOp { .. }
            | varn_types::ssa::SsaOp::ArrayPush { .. }
            | varn_types::ssa::SsaOp::This
            | varn_types::ssa::SsaOp::GetFixedField { .. }
            | varn_types::ssa::SsaOp::SetFixedField { .. }
            | varn_types::ssa::SsaOp::MakeClass { .. }
            | varn_types::ssa::SsaOp::DeclareLayout { .. }
            | varn_types::ssa::SsaOp::DefineMethod { .. }
            | varn_types::ssa::SsaOp::GetSuper { .. }
            | varn_types::ssa::SsaOp::LoadGlobal(_)
            | varn_types::ssa::SsaOp::StoreGlobal { .. }
            | varn_types::ssa::SsaOp::BuildTuple { .. }
            | varn_types::ssa::SsaOp::BuildArraySpread { .. }
            | varn_types::ssa::SsaOp::BuildObjectSpread { .. }
            | varn_types::ssa::SsaOp::ObjectMerge { .. }
            | varn_types::ssa::SsaOp::ObjectRest { .. }
            | varn_types::ssa::SsaOp::GetPropertyMaybe { .. }
            | varn_types::ssa::SsaOp::AssertNotNull { .. }
            | varn_types::ssa::SsaOp::BindMethod { .. }
            | varn_types::ssa::SsaOp::ArrayExtend { .. }
            | varn_types::ssa::SsaOp::WrapSpread { .. }
            | varn_types::ssa::SsaOp::Range { .. }
            | varn_types::ssa::SsaOp::GetSymbol { .. }
            | varn_types::ssa::SsaOp::IterCall { .. }
            | varn_types::ssa::SsaOp::SuperCall { .. }
            | varn_types::ssa::SsaOp::SuperMethodCall { .. }
            | varn_types::ssa::SsaOp::ExtensionCall { .. }
            | varn_types::ssa::SsaOp::CallSpread { .. }
            | varn_types::ssa::SsaOp::LoadModule { .. }
            | varn_types::ssa::SsaOp::ModuleSlot { .. }
            | varn_types::ssa::SsaOp::StoreModuleSlot { .. }
            | varn_types::ssa::SsaOp::Await { .. }
            | varn_types::ssa::SsaOp::Spawn { .. }
            | varn_types::ssa::SsaOp::Yield { .. }
            | varn_types::ssa::SsaOp::Dispose { .. } => {}
        }
    }
    None
}

pub(super) fn try_entry_stacks(
    ssa: &SsaProto,
    rpo: &[usize],
    reached: &[bool],
) -> Option<Vec<Vec<usize>>> {
    let n = ssa.blocks.len();
    let mut entry: Vec<Option<Vec<usize>>> = vec![None; n];
    entry[ssa.entry as usize] = Some(Vec::new());
    for _ in 0..n + 2 {
        let mut changed = false;
        for &b in rpo {
            if !reached[b] {
                continue;
            }
            let Some(cur) = entry[b].clone() else {
                continue;
            };
            for (ti, inst) in ssa.blocks[b].insts.iter().enumerate() {
                if let varn_types::ssa::SsaOp::Try { catch_ip, .. } = &inst.op {
                    let e = ssa.catch_at(*catch_ip as usize)?;
                    let h = e.block as usize;
                    if h >= n {
                        return None;
                    }
                    let outer = stack_before_try(ssa, &cur, b, ti)?;
                    match &entry[h] {
                        None => {
                            entry[h] = Some(outer);
                            changed = true;
                        }
                        Some(prev) if *prev != outer => return None,
                        _ => {}
                    }
                }
            }
            let exit = block_exit(ssa, &cur, b)?;
            for s in normal_succs(ssa, b) {
                if s >= n || !reached[s] {
                    continue;
                }
                match &entry[s] {
                    None => {
                        entry[s] = Some(exit.clone());
                        changed = true;
                    }
                    Some(prev) if *prev != exit => return None,
                    _ => {}
                }
            }
        }
        if !changed {
            let mut out = Vec::with_capacity(n);
            for (i, e) in entry.into_iter().enumerate() {
                if reached[i] {
                    out.push(e?);
                } else {
                    out.push(Vec::new());
                }
            }
            return Some(out);
        }
    }
    None
}
