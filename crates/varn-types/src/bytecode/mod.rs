//! The bytecode's instruction encoding, read from one table.
//!
//! [`layout`] states, per opcode, which byte or word of an instruction holds
//! what. Everything that walks `chunk.code` without executing it reads that
//! table through this module: [`decode`] (length, defined and used
//! registers, call windows — backend register allocation, slot-kind
//! analysis, the JIT's pre-scan and register mapping), [`remap_registers`]
//! (the register coalescer's renumbering) and [`disasm`] (listings for the
//! CLI, the debugger and the editor). Hand-rolled `ip += N` width tables are
//! how the JIT once silently rejected whole functions, how `Spawn` shipped
//! with three incompatible encodings, and how the listings drifted out of
//! step with the stream they listed.

pub mod disasm;
mod layout;
mod operand;
mod remap;

pub use layout::layout;
pub use operand::{Access, At, Byte, ConstKind, Half, ImmKind, Layout, Operand, RunKind};
pub use remap::remap_registers;

use crate::chunk::PoolEntry;

pub struct InstrInfo {
    /// Total instruction length in code words, including the opcode word.
    pub len: usize,

    /// Register defined (written) by this instruction, if any.
    pub def: Option<u8>,

    /// Registers read by this instruction.
    pub uses: Vec<u8>,

    /// `(arg_start, arg_count)` window for call-shaped instructions that
    /// require their arguments contiguous on the register file.
    pub call_args: Option<(u8, u8)>,

    /// Control-flow or otherwise unanalyzable instruction: walkers must
    /// treat every register as potentially live across it.
    pub opaque: bool,
}

/// The shape of the instruction at `offset`, as register walkers need it.
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
