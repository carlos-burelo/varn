












pub mod disasm;
mod layout;
mod operand;
mod remap;

pub use layout::layout;
pub use operand::{Access, At, Byte, ConstKind, Half, ImmKind, Layout, Operand, RunKind};
pub use remap::remap_registers;

use crate::chunk::PoolEntry;

pub struct InstrInfo {
    
    pub len: usize,

    
    pub def: Option<u8>,

    
    pub uses: Vec<u8>,

    
    
    pub call_args: Option<(u8, u8)>,

    
    
    pub opaque: bool,
}


pub fn decode(code: &[u16], offset: usize, constants: &[PoolEntry]) -> Option<InstrInfo> {
    let layout = layout(code, offset, constants)?;
    let mut info = InstrInfo {
        len: layout.len,
        def: None,
        uses: Vec::new(),
        call_args: None,
        opaque: layout.opaque(),
    };
    for operand in &layout.operands {
        match *operand {
            Operand::Reg { at, access } => {
                let reg = at.read(code, offset);
                if access != Access::Write {
                    info.uses.push(reg);
                }
                if access != Access::Read {
                    debug_assert!(info.def.is_none(), "{:?} defines two registers", layout.op);
                    info.def = Some(reg);
                }
            }
            Operand::Run { start, count, kind } => {
                let start = start.read(code, offset);
                info.uses
                    .extend((0..count).map(|i| start.wrapping_add(i as u8)));
                if kind == RunKind::CallArgs {
                    info.call_args = Some((start, count as u8));
                }
            }
            Operand::Fixed { reg, access } => {
                if access != Access::Write {
                    info.uses.push(reg);
                }
            }
            Operand::Const { .. } | Operand::Imm { .. } | Operand::Jump { .. } => {}
        }
    }
    Some(info)
}

#[cfg(test)]
mod tests;
