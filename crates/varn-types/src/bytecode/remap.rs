//! Renumbering an instruction stream's registers.

use std::collections::BTreeSet;

use super::{layout, Byte, Operand};
use crate::chunk::PoolEntry;

/// Rename every register operand in `code` through `map`.
///
/// The operand positions come from the layout table, so exactly the
/// register bytes change: constants, slots, counts and immediates that
/// share a word with a register keep their value. A run of registers is
/// renamed by its first register — the allocator keeps a run contiguous.
/// Registers fixed by convention (`this`) are never renamed. A byte is
/// renamed once, however many operands name it (an `Intrinsic`'s
/// destination is also the start of its argument window).
pub fn remap_registers(code: &mut [u16], constants: &[PoolEntry], map: impl Fn(u8) -> u8) {
    let mut offset = 0;
    while offset < code.len() {
        let Some(layout) = layout(code, offset, constants) else {
            break;
        };
        let registers: BTreeSet<Byte> = layout
            .operands
            .iter()
            .filter_map(|o| match *o {
                Operand::Reg { at, .. } | Operand::Run { start: at, .. } => Some(at),
                _ => None,
            })
            .filter(|at| offset + at.word < code.len())
            .collect();
        for at in registers {
            let old = at.read(code, offset);
            at.write(code, offset, map(old));
        }
        offset += layout.len;
    }
}
