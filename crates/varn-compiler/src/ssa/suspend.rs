use super::emit::var_reg;
use super::ir::{InstKind, SsaFunc, Value, VarId};
use super::liveness::Liveness;

#[derive(Debug, Clone)]
pub struct SuspendPoint {
    pub operand: Value,
    pub live: Vec<Value>,
}

pub fn analyze(ssa: &SsaFunc) -> Vec<SuspendPoint> {
    let lv = Liveness::analyze(ssa);
    let mut out = Vec::new();

    for (b, block) in ssa.blocks.iter().enumerate() {
        for (i, inst) in block.insts.iter().enumerate() {
            let operand = match &inst.kind {
                InstKind::Await { operand } | InstKind::Yield { operand } => *operand,
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
                | InstKind::MapGetIndex { .. }
                | InstKind::SetProperty { .. }
                | InstKind::SetFixedField { .. }
                | InstKind::SetIndex { .. }
                | InstKind::ArraySetIndex { .. }
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
                | InstKind::GetSymbol { .. }
                | InstKind::IterCall { .. }
                | InstKind::GetSuper { .. }
                | InstKind::SuperCall { .. }
                | InstKind::SuperMethodCall { .. }
                | InstKind::ExtensionCall { .. }
                | InstKind::CallSpread { .. }
                | InstKind::BuildArraySpread { .. }
                | InstKind::BuildObjectSpread { .. } => continue,
            };
            out.push(SuspendPoint {
                operand,
                live: lv.live_after(ssa, b, i),
            });
        }
    }
    out
}

pub fn suspend_live_regs(
    ssa: &SsaFunc,
    reg: &[u8],
    inst_next: &[Vec<usize>],
    nparams: usize,
) -> Vec<(u32, Vec<u16>)> {
    let lv = Liveness::analyze(ssa);
    let handlers = try_handlers(ssa);
    let homes = captured_home_regs(ssa, nparams);
    let mut table = Vec::new();
    for (b, block) in ssa.blocks.iter().enumerate() {
        for (i, inst) in block.insts.iter().enumerate() {
            match &inst.kind {
                InstKind::Await { .. } | InstKind::Yield { .. } => {}
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
                | InstKind::MapGetIndex { .. }
                | InstKind::SetProperty { .. }
                | InstKind::SetFixedField { .. }
                | InstKind::SetIndex { .. }
                | InstKind::ArraySetIndex { .. }
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
                | InstKind::GetSymbol { .. }
                | InstKind::IterCall { .. }
                | InstKind::GetSuper { .. }
                | InstKind::SuperCall { .. }
                | InstKind::SuperMethodCall { .. }
                | InstKind::ExtensionCall { .. }
                | InstKind::CallSpread { .. }
                | InstKind::BuildArraySpread { .. }
                | InstKind::BuildObjectSpread { .. } => continue,
            }
            let live = resume_live(ssa, &lv, &handlers, b, i);
            let mut regs: Vec<u16> = Vec::with_capacity(live.len());
            let mut ok = true;
            for v in &live {
                match reg.get(v.0 as usize) {
                    Some(&r) => regs.push(r as u16),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                continue;
            }
            regs.push(0);
            regs.extend(homes.iter().copied());
            let ip = inst_next
                .get(b)
                .and_then(|row| row.get(i))
                .copied()
                .unwrap_or(usize::MAX);
            let Ok(resume_ip) = u32::try_from(ip) else {
                continue;
            };
            if resume_ip == u32::MAX {
                continue;
            }
            regs.sort_unstable();
            regs.dedup();
            table.push((resume_ip, regs));
        }
    }
    table.sort_by_key(|(ip, _)| *ip);
    table
}

pub fn try_handlers(ssa: &SsaFunc) -> Vec<usize> {
    let mut handlers: Vec<usize> = Vec::new();
    for block in &ssa.blocks {
        for inst in &block.insts {
            if let InstKind::Try { handler } = &inst.kind {
                let h = handler.0 as usize;
                if !handlers.contains(&h) {
                    handlers.push(h);
                }
            }
        }
    }
    handlers
}

fn captured_home_regs(ssa: &SsaFunc, nparams: usize) -> Vec<u16> {
    let mut vars: Vec<VarId> = Vec::new();
    let mut note = |v: VarId| {
        if !vars.contains(&v) {
            vars.push(v);
        }
    };
    for block in &ssa.blocks {
        for inst in &block.insts {
            match &inst.kind {
                InstKind::LoadCaptured { var } | InstKind::StoreCaptured { var, .. } => note(*var),
                InstKind::CloseUpvalues { targets } => {
                    for v in targets {
                        note(*v);
                    }
                }
                InstKind::Dispose { target, .. } => {
                    note(VarId::Local(crate::hir::LocalId(target.0)))
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
                | InstKind::MapGetIndex { .. }
                | InstKind::SetProperty { .. }
                | InstKind::SetFixedField { .. }
                | InstKind::SetIndex { .. }
                | InstKind::ArraySetIndex { .. }
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
                | InstKind::MakeClass { .. }
                | InstKind::DeclareLayout { .. }
                | InstKind::DefineStatic { .. }
                | InstKind::DefineMethod { .. }
                | InstKind::DefineAccessor { .. }
                | InstKind::MakeEnumVariant { .. }
                | InstKind::Try { .. }
                | InstKind::PopTry
                | InstKind::CatchParam { .. }
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
                | InstKind::GetEnumTag { .. }
                | InstKind::IsArray { .. }
                | InstKind::StrLength { .. }
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
                | InstKind::BuildObjectSpread { .. } => {}
            }
        }
    }
    vars.into_iter()
        .map(|v| var_reg(v, nparams) as u16)
        .collect()
}

pub fn resume_live(
    ssa: &SsaFunc,
    lv: &Liveness,
    handlers: &[usize],
    b: usize,
    i: usize,
) -> Vec<Value> {
    let mut live = lv.live_after(ssa, b, i);
    for &h in handlers {
        if let Some(live_in) = lv.live_in.get(h) {
            for &v in live_in {
                live.push(Value(v));
            }
        }
    }
    live.sort_unstable_by_key(|v| v.0);
    live.dedup();
    live
}
