


use super::super::ir::{InstKind, SsaFunc, VarId};
use super::slot_kind_of;
use crate::OptError;

type Result<T> = std::result::Result<T, OptError>;


pub(super) struct Assignment {
    
    pub reg: Vec<u8>,
    pub scratch: u8,
    pub null_reg: u8,
    pub call_base: u8,
    pub register_count: u16,
    
    
    pub liveness: crate::ssa::liveness::Liveness,
}

pub(super) fn assign_registers(ssa: &SsaFunc, nparams: usize) -> Result<Assignment> {
    let mut reg = vec![0u8; ssa.values.len()];
    let mut assigned = vec![false; ssa.values.len()];

    let entry_params = &ssa.blocks[ssa.entry.0 as usize].params;
    if entry_params.len() != nparams {
        return Err(OptError::Unsupported("ssa-emit: entry params != arity"));
    }
    for (i, p) in entry_params.iter().enumerate() {
        reg[p.0 as usize] = (1 + i) as u8;
        assigned[p.0 as usize] = true;
    }

    let nvals = ssa.values.len();
    let order = super::emission_order(ssa);
    let lv = crate::ssa::liveness::Liveness::analyze_ordered(ssa, &order);
    let def = &lv.def;
    let end = &lv.end;

    let mut base = 1 + nparams as u32;
    for block in &ssa.blocks {
        for inst in &block.insts {
            if let InstKind::LoadCaptured { var } | InstKind::StoreCaptured { var, .. } = &inst.kind
            {
                base = base.max(var_reg(*var, nparams) as u32 + 1);
            }
        }
    }
    let mut order: Vec<usize> = (0..nvals)
        .filter(|&v| !assigned[v] && def[v] != u32::MAX)
        .collect();
    order.sort_by_key(|&v| def[v]);
    let mut max_call = 0u32;
    for block in &ssa.blocks {
        for inst in &block.insts {
            let t = match &inst.kind {
                InstKind::Call { args, .. } => args.len() as u32 + 1,
                InstKind::SelfCall { args } => args.len() as u32 + 1,
                InstKind::MethodCall { args, .. } => args.len() as u32,

                InstKind::BuildArray { elements, .. } => elements.len() as u32,
                InstKind::BuildTuple { elements } => elements.len() as u32,
                InstKind::BuildMap { pairs } => (pairs.len() * 2) as u32,
                
                
                InstKind::BuildObject { pairs } | InstKind::BuildRecord { pairs } => {
                    pairs.len() as u32
                }

                
                
                
                
                
                InstKind::IntrinsicCall { args, .. } => args.len() as u32 + 1,
                InstKind::CallNativeOp { args, .. } => args.len() as u32 + 1,

                InstKind::IterCall { .. } => 1,

                InstKind::SuperCall { args } => args.len() as u32 + 2,
                InstKind::SuperMethodCall { args, .. } => args.len() as u32 + 1,

                InstKind::ExtensionCall { args, .. } => args.len() as u32 + 2,
                InstKind::CallSpread { args, .. } => args.len() as u32 + 1,

                
                InstKind::BuildArraySpread { .. }
                | InstKind::ConstInt(_)
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
                | InstKind::IsNull { .. }
                | InstKind::Cast { .. }
                | InstKind::Convert { .. }
                | InstKind::ObjectRest { .. }
                | InstKind::ToString { .. }
                | InstKind::BuildStr { .. }
                | InstKind::MakeClosure { .. }
                | InstKind::LoadCaptured { .. }
                | InstKind::StoreCaptured { .. }
                | InstKind::MakeClass { .. }
                | InstKind::DeclareLayout { .. }
                | InstKind::AllocInstance { .. }
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
                | InstKind::GetSuper { .. }
                | InstKind::BuildObjectSpread { .. } => 0,
            };
            max_call = max_call.max(t);
        }
    }

    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    let kind_of_v = |v: usize| slot_kind_of(ssa.values[v].ty);
    
    
    let ceiling = 256u32.saturating_sub(2 + max_call);
    let mut next: u32 = base;
    
    let mut free: [Vec<u32>; POOLS] = Default::default();
    
    let mut active: Vec<(u32, u32, usize)> = Vec::new();
    for &v in &order {
        let d = def[v];
        let vk = pool_index(kind_of_v(v));
        let mut i = 0;
        while i < active.len() {
            if active[i].0 < d {
                let (_, r, rk) = active[i];
                free[rk].push(r);
                active.swap_remove(i);
            } else {
                i += 1;
            }
        }
        let r = if let Some(m) = free[vk].iter().copied().min() {
            free[vk].retain(|&x| x != m);
            m
        } else if next < ceiling {
            let r = next;
            next += 1;
            r
        } else {
            
            
            let borrowed = free
                .iter()
                .enumerate()
                .filter_map(|(k, p)| p.iter().copied().min().map(|m| (m, k)))
                .min();
            match borrowed {
                Some((m, k)) => {
                    free[k].retain(|&x| x != m);
                    m
                }
                None => {
                    let r = next;
                    next += 1;
                    r
                }
            }
        };
        if r > 255 {
            return Err(OptError::Unsupported(
                "ssa-emit: register count exceeds 255",
            ));
        }
        reg[v] = r as u8;
        assigned[v] = true;
        active.push((end[v], r, vk));
    }

    let total = next + 2 + max_call;
    if total > 256 {
        return Err(OptError::Unsupported(
            "ssa-emit: register count exceeds 255",
        ));
    }
    let scratch = next as u8;
    let null_reg = (next + 1) as u8;
    let call_base = (next + 2) as u8;
    let register_count = total.max(1) as u16;
    Ok(Assignment {
        reg,
        scratch,
        null_reg,
        call_base,
        register_count,
        liveness: lv,
    })
}

pub(crate) fn var_reg(var: VarId, nparams: usize) -> u8 {
    match var {
        VarId::Param(i) => (1 + i) as u8,
        VarId::Local(id) => (1 + nparams + id.0 as usize) as u8,
    }
}


const POOLS: usize = 6;


fn pool_index(k: varn_types::register_meta::SlotKind) -> usize {
    use varn_types::register_meta::SlotKind;
    match k {
        SlotKind::Int => 0,
        SlotKind::Float => 1,
        SlotKind::Bool => 2,
        SlotKind::Str => 3,
        SlotKind::Ref => 4,
        SlotKind::Dynamic => 5,
    }
}
