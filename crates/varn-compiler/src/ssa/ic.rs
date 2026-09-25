//! Inline-cache slots: one per call-site/property site, numbered once.
//!
//! The bytecode emitter bakes a slot into each site's operand and the portable
//! SSA carries the same slot for the JIT, which reads and fills the same
//! `ic_cache` entry. Both read the numbering from here — computed in the
//! order the emitter walks the blocks, over exactly the instructions it emits
//! with a cache — so they cannot drift apart.

use super::ir::{InstKind, SsaFunc};
use crate::OptError;

/// The first slot of every instruction that owns some, by `[block][inst]`;
/// an instruction's slots are consecutive from there.
pub(crate) struct IcSlots {
    slots: Vec<Vec<Option<u8>>>,
    count: u16,
}

impl IcSlots {
    /// Numbers the sites of `ssa`, walking blocks in `order` (the emission
    /// order).
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
                // Every slot, the last included, must fit the operand byte.
                u8::try_from(count + n - 1).map_err(|_| too_many())?;
                slots[b][i] = Some(count as u8);
                count += n;
            }
        }
        Ok(Self { slots, count })
    }

    /// The first slot of instruction `inst` of block `block`, if it owns any.
    pub(crate) fn of(&self, block: usize, inst: usize) -> Option<u8> {
        self.slots.get(block)?.get(inst).copied().flatten()
    }

    /// How many slots the function has (`FunctionProto::cache_count`).
    pub(crate) fn count(&self) -> u16 {
        self.count
    }
}

/// How many cache sites an instruction is emitted with. A value instruction
/// whose destination DCE dropped is still emitted only when it is droppable
/// (its effect must run), which is the emitter's own rule. An object literal
/// with spreads stores each keyed part through its own `SetProperty` site.
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
