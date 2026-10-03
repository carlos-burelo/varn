use crate::ssa::ir::Value;
use crate::OptError;
use std::sync::Arc;
use varn_core::OpCode;
use varn_types::chunk::Chunk;

type Result<T> = std::result::Result<T, OptError>;

pub(super) fn emit_build_array(
    chunk: &mut Chunk,
    d: u8,
    elements: &[Value],
    reg: &[u8],
    call_base: u8,
    line: u32,
) {
    for (i, e) in elements.iter().enumerate() {
        chunk.emit_rr(OpCode::Move, call_base + i as u8, reg[e.0 as usize], line);
    }
    chunk.emit(OpCode::BuildArray, line);
    chunk.write(Chunk::pack(d, call_base), line);
    chunk.write(Chunk::pack(elements.len() as u8, 0), line);
}

pub(super) fn emit_build_tuple(
    chunk: &mut Chunk,
    d: u8,
    elements: &[Value],
    reg: &[u8],
    call_base: u8,
    line: u32,
) {
    for (i, e) in elements.iter().enumerate() {
        chunk.emit_rr(OpCode::Move, call_base + i as u8, reg[e.0 as usize], line);
    }
    chunk.emit(OpCode::BuildTuple, line);
    chunk.write(Chunk::pack(d, call_base), line);
    chunk.write(Chunk::pack(elements.len() as u8, 0), line);
}

pub(super) fn emit_build_object(
    chunk: &mut Chunk,
    d: u8,
    pairs: &[(Arc<str>, Value)],
    reg: &[u8],
    call_base: u8,
    line: u32,
) {
    let count = pairs.len();
    let mut is_contiguous = count > 0;
    let mut start_reg = call_base;
    if count > 0 {
        let first = reg[pairs[0].1 .0 as usize];
        for (i, (_, v)) in pairs.iter().enumerate() {
            if reg[v.0 as usize] != first + i as u8 {
                is_contiguous = false;
                break;
            }
        }
        if is_contiguous {
            start_reg = first;
        } else {
            for (i, (_, v)) in pairs.iter().enumerate() {
                chunk.emit_rr(OpCode::Move, call_base + i as u8, reg[v.0 as usize], line);
            }
        }
    }
    let keys = pairs.iter().map(|(k, _)| Arc::from(k.as_ref())).collect();
    let shape_idx = chunk.add_shape(keys);
    chunk.emit(OpCode::BuildObjectWithShape, line);
    chunk.write(Chunk::pack(d, start_reg), line);
    chunk.write(shape_idx, line);
}

pub(super) fn emit_build_record(
    chunk: &mut Chunk,
    d: u8,
    pairs: &[(Arc<str>, Value)],
    reg: &[u8],
    call_base: u8,
    line: u32,
) {
    let count = pairs.len();
    let mut is_contiguous = count > 0;
    let mut start_reg = call_base;
    if count > 0 {
        let first = reg[pairs[0].1 .0 as usize];
        for (i, (_, v)) in pairs.iter().enumerate() {
            if reg[v.0 as usize] != first + i as u8 {
                is_contiguous = false;
                break;
            }
        }
        if is_contiguous {
            start_reg = first;
        } else {
            for (i, (_, v)) in pairs.iter().enumerate() {
                chunk.emit_rr(OpCode::Move, call_base + i as u8, reg[v.0 as usize], line);
            }
        }
    }
    let keys = pairs.iter().map(|(k, _)| Arc::from(k.as_ref())).collect();
    let shape_idx = chunk.add_shape(keys);
    chunk.emit(OpCode::BuildRecord, line);
    chunk.write(Chunk::pack(d, start_reg), line);
    chunk.write(shape_idx, line);
}

pub(super) fn emit_build_map(
    chunk: &mut Chunk,
    d: u8,
    pairs: &[(Value, Value)],
    reg: &[u8],
    call_base: u8,
    line: u32,
) {
    for (i, (k, v)) in pairs.iter().enumerate() {
        chunk.emit_rr(
            OpCode::Move,
            call_base + (i * 2) as u8,
            reg[k.0 as usize],
            line,
        );
        chunk.emit_rr(
            OpCode::Move,
            call_base + (i * 2 + 1) as u8,
            reg[v.0 as usize],
            line,
        );
    }
    chunk.emit(OpCode::BuildMap, line);
    chunk.write(Chunk::pack(d, call_base), line);
    chunk.write(Chunk::pack(pairs.len() as u8, 0), line);
}

pub(super) fn emit_build_str(chunk: &mut Chunk, d: u8, parts: &[Value], reg: &[u8], line: u32) {
    chunk.write(Chunk::pack_op(OpCode::BuildStr, d), line);
    chunk.write(Chunk::pack(parts.len() as u8, 0), line);
    for p in parts {
        chunk.write(Chunk::pack(reg[p.0 as usize], 0), line);
    }
}

pub(super) fn emit_build_array_spread(
    chunk: &mut Chunk,
    d: u8,
    elements: &[(Value, bool)],
    reg: &[u8],
    call_base: u8,
    line: u32,
) {
    chunk.emit(OpCode::BuildArray, line);
    chunk.write(Chunk::pack(d, call_base), line);
    chunk.write(Chunk::pack(0, 0), line);
    for (v, spread) in elements {
        let op = if *spread {
            OpCode::ArrayExtend
        } else {
            OpCode::ArrayPush
        };
        chunk.emit_rr(op, d, reg[v.0 as usize], line);
    }
}

pub(super) fn emit_build_object_spread(
    chunk: &mut Chunk,
    d: u8,
    parts: &[(Option<Arc<str>>, Value)],
    reg: &[u8],
    ic_slot: Option<u8>,
    line: u32,
) -> Result<()> {
    chunk.emit(OpCode::BuildObject, line);
    chunk.write(Chunk::pack(d, 0), line);
    let mut next_slot = ic_slot;
    for (key, v) in parts {
        match key {
            Some(k) => {
                let idx = chunk.add_str(k);
                let cs = next_slot
                    .ok_or(OptError::Unsupported("ssa-emit: cache site without a slot"))?;
                next_slot = cs.checked_add(1);
                chunk.emit_rrc_ic(OpCode::SetProperty, d, reg[v.0 as usize], idx, cs, line);
            }
            None => chunk.emit_rr(OpCode::ObjectMerge, d, reg[v.0 as usize], line),
        }
    }
    Ok(())
}

pub(super) fn emit_object_rest(
    chunk: &mut Chunk,
    d: u8,
    object: Value,
    skip_keys: &[Arc<str>],
    reg: &[u8],
    line: u32,
) {
    chunk.emit(OpCode::ObjectRest, line);
    chunk.write(Chunk::pack(d, reg[object.0 as usize]), line);
    chunk.write(Chunk::pack(skip_keys.len() as u8, 0), line);
    for k in skip_keys {
        let idx = chunk.add_str(k);
        chunk.write(idx, line);
    }
}
