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
        _ => 0,
    }
}
