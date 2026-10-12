use crate::ssa::ir::{InstKind, SsaFunc, Terminator};
use varn_types::ssa::{SsaEntryKind, SsaSecondaryEntry};

use super::Emitted;

pub(super) fn secondary_entries(ssa: &SsaFunc, emitted: &Emitted<'_>) -> Vec<SsaSecondaryEntry> {
    let mut out = loop_entries(ssa, emitted);
    out.extend(resume_entries(ssa, emitted));
    out.sort_by_key(|e| (e.ip, e.block));
    out
}

fn loop_entries(ssa: &SsaFunc, emitted: &Emitted<'_>) -> Vec<SsaSecondaryEntry> {
    let off = |b: usize| emitted.block_offset.get(b).copied().unwrap_or(usize::MAX);
    let mut is_header = vec![false; ssa.blocks.len()];
    for (b, block) in ssa.blocks.iter().enumerate() {
        let targets = match &block.term {
            Terminator::Jump { target, .. } => vec![target.0 as usize],
            Terminator::Branch {
                then_blk, else_blk, ..
            } => vec![then_blk.0 as usize, else_blk.0 as usize],
            Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => Vec::new(),
        };
        for t in targets {
            if off(b) != usize::MAX && off(t) <= off(b) {
                is_header[t] = true;
            }
        }
    }
    is_header
        .iter()
        .enumerate()
        .filter(|(_, h)| **h)
        .filter_map(|(b, _)| {
            let ip = u32::try_from(off(b)).ok()?;
            let mut live: Vec<u32> = emitted.liveness.live_in[b].iter().copied().collect();
            live.sort_unstable();
            Some(SsaSecondaryEntry {
                block: b as u32,
                ip,
                live,
                kind: SsaEntryKind::Loop,
            })
        })
        .collect()
}

fn resume_entries(ssa: &SsaFunc, emitted: &Emitted<'_>) -> Vec<SsaSecondaryEntry> {
    let mut out = Vec::new();
    for (b, block) in ssa.blocks.iter().enumerate() {
        let cont = match &block.term {
            Terminator::Jump { target, .. } => target.0,
            Terminator::Branch { .. } => continue,
            Terminator::Return(_) => continue,
            Terminator::Throw(_) => continue,
            Terminator::Unreachable => continue,
        };
        for (i, inst) in block.insts.iter().enumerate() {
            if !is_suspend_site(&inst.kind) {
                continue;
            }
            let Some(next) = emitted.inst_next.get(b).and_then(|row| row.get(i)).copied() else {
                continue;
            };
            if next == usize::MAX {
                continue;
            }
            let Ok(ip) = u32::try_from(next) else {
                continue;
            };
            if ip == u32::MAX {
                continue;
            }
            let mut live: Vec<u32> = emitted
                .liveness
                .live_after(ssa, b, i)
                .into_iter()
                .map(|v| v.0)
                .collect();
            live.sort_unstable();
            out.push(SsaSecondaryEntry {
                block: cont,
                ip,
                live,
                kind: SsaEntryKind::Resume,
            });
        }
    }
    out
}

fn is_suspend_site(kind: &InstKind) -> bool {
    match kind {
        InstKind::Await { .. } => true,
        InstKind::Yield { .. } => true,
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
        | InstKind::BuildObjectSpread { .. } => false,
    }
}
