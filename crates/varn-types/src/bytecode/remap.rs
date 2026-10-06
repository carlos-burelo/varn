use std::collections::BTreeSet;

use super::{layout, Byte, Operand};
use crate::chunk::PoolEntry;

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
