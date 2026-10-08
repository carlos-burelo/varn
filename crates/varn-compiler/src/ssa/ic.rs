use super::ir::{InstKind, SsaFunc};
use crate::OptError;

pub(crate) struct IcSlots {
    slots: Vec<Vec<Option<u8>>>,
    count: u16,
}

impl IcSlots {
    pub(crate) fn number(ssa: &SsaFunc, order: &[usize]) -> Result<Self, OptError> {
        let mut slots: Vec<Vec<Option<u8>>> = ssa
            .blocks
            .iter()
            .map(|b| vec![None; b.insts.len()])
            .collect();
        let mut count: u16 = 0;
        for &b in order {
            for (i, inst) in ssa.blocks[b].insts.iter().enumerate() {
                let n = sites(&inst.kind, inst.dest.is_some());
                if n == 0 {
                    continue;
                }
                let too_many = || OptError::Unsupported("ssa-emit: too many inline-cache sites");

                u8::try_from(count + n - 1).map_err(|_| too_many())?;
                slots[b][i] = Some(count as u8);
                count += n;
            }
        }
        Ok(Self { slots, count })
    }

    pub(crate) fn of(&self, block: usize, inst: usize) -> Option<u8> {
        self.slots.get(block)?.get(inst).copied().flatten()
    }

    pub(crate) fn count(&self) -> u16 {
        self.count
    }
}

fn sites(kind: &InstKind, has_dest: bool) -> u16 {
    match kind {
        InstKind::SetProperty { .. } | InstKind::Dispose { .. } => 1,
        InstKind::GetProperty { .. } | InstKind::MethodCall { .. } => {
            u16::from(has_dest || crate::passes::dce::dest_droppable(kind))
        }
        InstKind::BuildObjectSpread { parts } if has_dest => {
            parts.iter().filter(|(key, _)| key.is_some()).count() as u16
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
        | InstKind::GetFixedField { .. }
        | InstKind::GetIndex { .. }
        | InstKind::ArrayGetIndex { .. }
        | InstKind::MapGetIndex { .. }
        | InstKind::SetFixedField { .. }
        | InstKind::SetIndex { .. }
        | InstKind::ArraySetIndex { .. }
        | InstKind::MapSetIndex { .. }
        | InstKind::ArrayPush { .. }
        | InstKind::ObjectMerge { .. }
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
        | InstKind::BuildObjectSpread { .. } => 0,
    }
}
