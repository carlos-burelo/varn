use rustc_hash::FxHashMap as HashMap;
use varn_core::OpCode;
use varn_types::bytecode::{decode, remap_registers};
use varn_types::chunk::PoolEntry;

/// Rewrite `code` onto the coalesced registers.
///
/// Which bytes are registers is the layout table's to say
/// ([`varn_types::bytecode::layout`]); a move whose ends the colouring put
/// in one register is then dead and becomes two `Nop`s.
pub(crate) fn remap_bytecode(code: &mut [u16], constants: &[PoolEntry], mapping: &HashMap<u8, u8>) {
    remap_registers(code, constants, |r| mapping.get(&r).copied().unwrap_or(r));

    let mut offset = 0;
    while let Some(info) = decode(code, offset, constants) {
        if OpCode::from_u16(code[offset]) == Some(OpCode::Move)
            && info.def.is_some()
            && info.def == info.uses.first().copied()
        {
            code[offset] = OpCode::Nop as u16;
            code[offset + 1] = OpCode::Nop as u16;
        }
        offset += info.len;
    }
}
