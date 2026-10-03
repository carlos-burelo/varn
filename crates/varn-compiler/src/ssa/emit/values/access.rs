use crate::ssa::ir::{Value, VarId};
use crate::OptError;
use std::sync::Arc;
use varn_core::OpCode;
use varn_types::chunk::Chunk;

type Result<T> = std::result::Result<T, OptError>;

pub(super) fn emit_load_global(chunk: &mut Chunk, d: u8, name: &Arc<str>, line: u32) {
    let idx = chunk.add_str(name);
    chunk.emit_rc(OpCode::LoadGlobal, d, idx, line);
}

pub(super) fn emit_load_global_idx(chunk: &mut Chunk, d: u8, slot: u32, line: u32) -> Result<()> {
    let slot = u16::try_from(slot)
        .map_err(|_| OptError::Unsupported("ssa-emit: global slot exceeds u16"))?;
    chunk.emit_rc(OpCode::LoadGlobalIdx, d, slot, line);
    Ok(())
}

pub(super) fn emit_load_native_global_idx(
    chunk: &mut Chunk,
    d: u8,
    slot: u32,
    line: u32,
) -> Result<()> {
    let slot = u16::try_from(slot)
        .map_err(|_| OptError::Unsupported("ssa-emit: native global slot exceeds u16"))?;
    chunk.emit_rc(OpCode::LoadNativeGlobalIdx, d, slot, line);
    Ok(())
}

pub(super) fn emit_load_upvalue(chunk: &mut Chunk, d: u8, uv: u32, line: u32) {
    chunk.emit(OpCode::LoadUpvalue, line);
    chunk.write(Chunk::pack(d, uv as u8), line);
}

pub(super) fn emit_load_captured(chunk: &mut Chunk, d: u8, var: VarId, nparams: usize, line: u32) {
    let src = super::super::regs::var_reg(var, nparams);
    chunk.emit_rr(OpCode::Move, d, src, line);
}

pub(super) fn emit_load_module(chunk: &mut Chunk, d: u8, source: &Arc<str>, line: u32) {
    let src_idx = chunk.add_str(source);
    chunk.emit_rc(OpCode::LoadModule, d, src_idx, line);
}

pub(super) fn emit_get_super(chunk: &mut Chunk, d: u8, name: &Arc<str>, line: u32) {
    let idx = chunk.add_str(name);
    chunk.emit_rc(OpCode::GetSuper, d, idx, line);
}

pub(super) fn emit_get_property(
    chunk: &mut Chunk,
    d: u8,
    object: Value,
    name: &Arc<str>,
    reg: &[u8],
    ic_slot: Option<u8>,
    line: u32,
) -> Result<()> {
    let idx = chunk.add_str(name);
    let cs = ic_slot.ok_or(OptError::Unsupported("ssa-emit: cache site without a slot"))?;
    chunk.emit_rrc_ic(
        OpCode::GetProperty,
        d,
        reg[object.0 as usize],
        idx,
        cs,
        line,
    );
    Ok(())
}

pub(super) fn emit_get_fixed_field(
    chunk: &mut Chunk,
    d: u8,
    object: Value,
    reg: &[u8],
    slot: u16,
    offset: u32,
    tag: varn_core::FieldAccess,
    line: u32,
) {
    let tag_byte = tag.encode();
    chunk.write(Chunk::pack_op(OpCode::GetFixedField, d), line);
    chunk.write(Chunk::pack(reg[object.0 as usize], tag_byte), line);
    chunk.write(slot, line);
    chunk.write(offset as u16, line);
}

pub(super) fn emit_get_index(
    chunk: &mut Chunk,
    d: u8,
    object: Value,
    index: Value,
    reg: &[u8],
    line: u32,
) {
    chunk.emit_rrr(
        OpCode::GetIndex,
        d,
        reg[object.0 as usize],
        reg[index.0 as usize],
        line,
    );
}

pub(super) fn emit_array_get_index(
    chunk: &mut Chunk,
    d: u8,
    object: Value,
    index: Value,
    reg: &[u8],
    line: u32,
) {
    chunk.emit_rrr(
        OpCode::ArrayGetIndex,
        d,
        reg[object.0 as usize],
        reg[index.0 as usize],
        line,
    );
}

pub(super) fn emit_map_get_index(
    chunk: &mut Chunk,
    d: u8,
    object: Value,
    index: Value,
    reg: &[u8],
    line: u32,
) {
    chunk.emit_rrr(
        OpCode::MapGetIndex,
        d,
        reg[object.0 as usize],
        reg[index.0 as usize],
        line,
    );
}

pub(super) fn emit_get_property_maybe(
    chunk: &mut Chunk,
    d: u8,
    object: Value,
    name: &Arc<str>,
    reg: &[u8],
    line: u32,
) {
    let idx = chunk.add_str(name);
    chunk.emit_rrc(
        OpCode::GetPropertyMaybe,
        d,
        reg[object.0 as usize],
        idx,
        line,
    );
}

pub(super) fn emit_module_slot(
    chunk: &mut Chunk,
    d: u8,
    object: Value,
    reg: &[u8],
    slot: u16,
    line: u32,
) {
    chunk.emit_rrc(
        OpCode::LoadModuleSlot,
        d,
        reg[object.0 as usize],
        slot,
        line,
    );
}

pub(super) fn emit_range(
    chunk: &mut Chunk,
    d: u8,
    start: Value,
    end: Value,
    inclusive: bool,
    reg: &[u8],
    line: u32,
) {
    let method = chunk.add_str(varn_core::well_known::RUNTIME_RANGE);
    let flag = if inclusive { 1u8 } else { 0u8 };
    chunk.emit(OpCode::InvokeRuntimeStatic, line);
    chunk.write(Chunk::pack(d, 0), line);
    chunk.write(method, line);
    chunk.write(Chunk::pack(2, reg[start.0 as usize]), line);
    chunk.write(Chunk::pack(reg[end.0 as usize], flag), line);
}
