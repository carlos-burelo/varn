use super::cfg::normal_succs;
use vn_types::ssa::SsaProto;

pub(super) fn block_exit(ssa: &SsaProto, entry: &[usize], b: usize) -> Option<Vec<usize>> {
    let mut stack = entry.to_vec();
    for inst in &ssa.blocks[b].insts {
        match &inst.op {
            vn_types::ssa::SsaOp::Try { catch_ip, .. } => {
                let e = ssa.catch_at(*catch_ip as usize)?;
                stack.push(e.block as usize);
            }
            vn_types::ssa::SsaOp::PopTry => {
                stack.pop()?;
            }
            vn_types::ssa::SsaOp::ConstInt(_)
            | vn_types::ssa::SsaOp::ConstFloat(_)
            | vn_types::ssa::SsaOp::ConstBool(_)
            | vn_types::ssa::SsaOp::ConstNull
            | vn_types::ssa::SsaOp::ConstStr(_)
            | vn_types::ssa::SsaOp::Binary { .. }
            | vn_types::ssa::SsaOp::Unary { .. }
            | vn_types::ssa::SsaOp::SelfCall { .. }
            | vn_types::ssa::SsaOp::Call { .. }
            | vn_types::ssa::SsaOp::AllocInstance { .. }
            | vn_types::ssa::SsaOp::LoadGlobalIdx(_)
            | vn_types::ssa::SsaOp::ArrayGetIndex { .. }
            | vn_types::ssa::SsaOp::BytesGetIndex { .. }
            | vn_types::ssa::SsaOp::ArraySetIndex { .. }
            | vn_types::ssa::SsaOp::BytesSetIndex { .. }
            | vn_types::ssa::SsaOp::ConstChar(_)
            | vn_types::ssa::SsaOp::ConstBigInt(_)
            | vn_types::ssa::SsaOp::ConstDecimal(_)
            | vn_types::ssa::SsaOp::MakeEnumVariant { .. }
            | vn_types::ssa::SsaOp::IntrinsicCall { .. }
            | vn_types::ssa::SsaOp::LoadNativeGlobalIdx(_)
            | vn_types::ssa::SsaOp::CatchParam { .. }
            | vn_types::ssa::SsaOp::StoreGlobalIdx { .. }
            | vn_types::ssa::SsaOp::MakeClosure { .. }
            | vn_types::ssa::SsaOp::LoadCaptured { .. }
            | vn_types::ssa::SsaOp::StoreCaptured { .. }
            | vn_types::ssa::SsaOp::LoadUpvalue(_)
            | vn_types::ssa::SsaOp::StoreUpvalue { .. }
            | vn_types::ssa::SsaOp::CloseUpvalues { .. }
            | vn_types::ssa::SsaOp::Cast { .. }
            | vn_types::ssa::SsaOp::Convert { .. }
            | vn_types::ssa::SsaOp::IsNull { .. }
            | vn_types::ssa::SsaOp::Typeof { .. }
            | vn_types::ssa::SsaOp::ToString { .. }
            | vn_types::ssa::SsaOp::IsArray { .. }
            | vn_types::ssa::SsaOp::GetEnumTag { .. }
            | vn_types::ssa::SsaOp::ObjectKeys { .. }
            | vn_types::ssa::SsaOp::BuildStr { .. }
            | vn_types::ssa::SsaOp::BuildArray { .. }
            | vn_types::ssa::SsaOp::BuildMap { .. }
            | vn_types::ssa::SsaOp::BuildObject { .. }
            | vn_types::ssa::SsaOp::GetProperty { .. }
            | vn_types::ssa::SsaOp::SetProperty { .. }
            | vn_types::ssa::SsaOp::GetIndex { .. }
            | vn_types::ssa::SsaOp::SetIndex { .. }
            | vn_types::ssa::SsaOp::ArrayLength { .. }
            | vn_types::ssa::SsaOp::StrLength { .. }
            | vn_types::ssa::SsaOp::BytesLength { .. }
            | vn_types::ssa::SsaOp::MethodCall { .. }
            | vn_types::ssa::SsaOp::CallNativeOp { .. }
            | vn_types::ssa::SsaOp::ArrayPush { .. }
            | vn_types::ssa::SsaOp::This
            | vn_types::ssa::SsaOp::GetFixedField { .. }
            | vn_types::ssa::SsaOp::SetFixedField { .. }
            | vn_types::ssa::SsaOp::MakeClass { .. }
            | vn_types::ssa::SsaOp::DeclareLayout { .. }
            | vn_types::ssa::SsaOp::DefineMethod { .. }
            | vn_types::ssa::SsaOp::GetSuper { .. }
            | vn_types::ssa::SsaOp::LoadGlobal(_)
            | vn_types::ssa::SsaOp::StoreGlobal { .. }
            | vn_types::ssa::SsaOp::BuildTuple { .. }
            | vn_types::ssa::SsaOp::BuildArraySpread { .. }
            | vn_types::ssa::SsaOp::BuildObjectSpread { .. }
            | vn_types::ssa::SsaOp::ObjectMerge { .. }
            | vn_types::ssa::SsaOp::ObjectRest { .. }
            | vn_types::ssa::SsaOp::GetPropertyMaybe { .. }
            | vn_types::ssa::SsaOp::AssertNotNull { .. }
            | vn_types::ssa::SsaOp::BindMethod { .. }
            | vn_types::ssa::SsaOp::ArrayExtend { .. }
            | vn_types::ssa::SsaOp::WrapSpread { .. }
            | vn_types::ssa::SsaOp::Range { .. }
            | vn_types::ssa::SsaOp::SuperCall { .. }
            | vn_types::ssa::SsaOp::SuperMethodCall { .. }
            | vn_types::ssa::SsaOp::ExtensionCall { .. }
            | vn_types::ssa::SsaOp::CallSpread { .. }
            | vn_types::ssa::SsaOp::LoadModule { .. }
            | vn_types::ssa::SsaOp::ModuleSlot { .. }
            | vn_types::ssa::SsaOp::StoreModuleSlot { .. }
            | vn_types::ssa::SsaOp::Await { .. }
            | vn_types::ssa::SsaOp::Spawn { .. }
            | vn_types::ssa::SsaOp::Yield { .. }
            | vn_types::ssa::SsaOp::Dispose { .. } => {}
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
            vn_types::ssa::SsaOp::Try { catch_ip, .. } => {
                let e = ssa.catch_at(*catch_ip as usize)?;
                stack.push(e.block as usize);
            }
            vn_types::ssa::SsaOp::PopTry => {
                stack.pop()?;
            }
            vn_types::ssa::SsaOp::ConstInt(_)
            | vn_types::ssa::SsaOp::ConstFloat(_)
            | vn_types::ssa::SsaOp::ConstBool(_)
            | vn_types::ssa::SsaOp::ConstNull
            | vn_types::ssa::SsaOp::ConstStr(_)
            | vn_types::ssa::SsaOp::Binary { .. }
            | vn_types::ssa::SsaOp::Unary { .. }
            | vn_types::ssa::SsaOp::SelfCall { .. }
            | vn_types::ssa::SsaOp::Call { .. }
            | vn_types::ssa::SsaOp::AllocInstance { .. }
            | vn_types::ssa::SsaOp::LoadGlobalIdx(_)
            | vn_types::ssa::SsaOp::ArrayGetIndex { .. }
            | vn_types::ssa::SsaOp::BytesGetIndex { .. }
            | vn_types::ssa::SsaOp::ArraySetIndex { .. }
            | vn_types::ssa::SsaOp::BytesSetIndex { .. }
            | vn_types::ssa::SsaOp::ConstChar(_)
            | vn_types::ssa::SsaOp::ConstBigInt(_)
            | vn_types::ssa::SsaOp::ConstDecimal(_)
            | vn_types::ssa::SsaOp::MakeEnumVariant { .. }
            | vn_types::ssa::SsaOp::IntrinsicCall { .. }
            | vn_types::ssa::SsaOp::LoadNativeGlobalIdx(_)
            | vn_types::ssa::SsaOp::CatchParam { .. }
            | vn_types::ssa::SsaOp::StoreGlobalIdx { .. }
            | vn_types::ssa::SsaOp::MakeClosure { .. }
            | vn_types::ssa::SsaOp::LoadCaptured { .. }
            | vn_types::ssa::SsaOp::StoreCaptured { .. }
            | vn_types::ssa::SsaOp::LoadUpvalue(_)
            | vn_types::ssa::SsaOp::StoreUpvalue { .. }
            | vn_types::ssa::SsaOp::CloseUpvalues { .. }
            | vn_types::ssa::SsaOp::Cast { .. }
            | vn_types::ssa::SsaOp::Convert { .. }
            | vn_types::ssa::SsaOp::IsNull { .. }
            | vn_types::ssa::SsaOp::Typeof { .. }
            | vn_types::ssa::SsaOp::ToString { .. }
            | vn_types::ssa::SsaOp::IsArray { .. }
            | vn_types::ssa::SsaOp::GetEnumTag { .. }
            | vn_types::ssa::SsaOp::ObjectKeys { .. }
            | vn_types::ssa::SsaOp::BuildStr { .. }
            | vn_types::ssa::SsaOp::BuildArray { .. }
            | vn_types::ssa::SsaOp::BuildMap { .. }
            | vn_types::ssa::SsaOp::BuildObject { .. }
            | vn_types::ssa::SsaOp::GetProperty { .. }
            | vn_types::ssa::SsaOp::SetProperty { .. }
            | vn_types::ssa::SsaOp::GetIndex { .. }
            | vn_types::ssa::SsaOp::SetIndex { .. }
            | vn_types::ssa::SsaOp::ArrayLength { .. }
            | vn_types::ssa::SsaOp::StrLength { .. }
            | vn_types::ssa::SsaOp::BytesLength { .. }
            | vn_types::ssa::SsaOp::MethodCall { .. }
            | vn_types::ssa::SsaOp::CallNativeOp { .. }
            | vn_types::ssa::SsaOp::ArrayPush { .. }
            | vn_types::ssa::SsaOp::This
            | vn_types::ssa::SsaOp::GetFixedField { .. }
            | vn_types::ssa::SsaOp::SetFixedField { .. }
            | vn_types::ssa::SsaOp::MakeClass { .. }
            | vn_types::ssa::SsaOp::DeclareLayout { .. }
            | vn_types::ssa::SsaOp::DefineMethod { .. }
            | vn_types::ssa::SsaOp::GetSuper { .. }
            | vn_types::ssa::SsaOp::LoadGlobal(_)
            | vn_types::ssa::SsaOp::StoreGlobal { .. }
            | vn_types::ssa::SsaOp::BuildTuple { .. }
            | vn_types::ssa::SsaOp::BuildArraySpread { .. }
            | vn_types::ssa::SsaOp::BuildObjectSpread { .. }
            | vn_types::ssa::SsaOp::ObjectMerge { .. }
            | vn_types::ssa::SsaOp::ObjectRest { .. }
            | vn_types::ssa::SsaOp::GetPropertyMaybe { .. }
            | vn_types::ssa::SsaOp::AssertNotNull { .. }
            | vn_types::ssa::SsaOp::BindMethod { .. }
            | vn_types::ssa::SsaOp::ArrayExtend { .. }
            | vn_types::ssa::SsaOp::WrapSpread { .. }
            | vn_types::ssa::SsaOp::Range { .. }
            | vn_types::ssa::SsaOp::SuperCall { .. }
            | vn_types::ssa::SsaOp::SuperMethodCall { .. }
            | vn_types::ssa::SsaOp::ExtensionCall { .. }
            | vn_types::ssa::SsaOp::CallSpread { .. }
            | vn_types::ssa::SsaOp::LoadModule { .. }
            | vn_types::ssa::SsaOp::ModuleSlot { .. }
            | vn_types::ssa::SsaOp::StoreModuleSlot { .. }
            | vn_types::ssa::SsaOp::Await { .. }
            | vn_types::ssa::SsaOp::Spawn { .. }
            | vn_types::ssa::SsaOp::Yield { .. }
            | vn_types::ssa::SsaOp::Dispose { .. } => {}
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
                if let vn_types::ssa::SsaOp::Try { catch_ip, .. } = &inst.op {
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
