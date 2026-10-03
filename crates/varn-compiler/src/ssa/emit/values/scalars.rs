use crate::hir::{HirBinOp, HirType, HirUnOp};
use crate::lower::binary_opcode;
use crate::ssa::ir::Value;
use crate::OptError;
use std::sync::Arc;
use varn_core::OpCode;
use varn_types::chunk::{Chunk, Literal, PoolEntry};
use varn_types::value::RuntimeSymbol;

type Result<T> = std::result::Result<T, OptError>;

pub(super) fn emit_const_int(chunk: &mut Chunk, d: u8, n: i64, line: u32) {
    chunk.emit_load_int(d, n, line);
}

pub(super) fn emit_const_float(chunk: &mut Chunk, d: u8, f: f64, line: u32) {
    let idx = chunk.add_constant(PoolEntry::Literal(Literal::Float(f)));
    chunk.emit_rc(OpCode::LoadConst, d, idx, line);
}

pub(super) fn emit_const_bool(chunk: &mut Chunk, d: u8, b: bool, line: u32) {
    let op = if b {
        OpCode::LoadTrue
    } else {
        OpCode::LoadFalse
    };
    chunk.emit_rr(op, d, 0, line);
}

pub(super) fn emit_const_str(chunk: &mut Chunk, d: u8, s: &Arc<str>, line: u32) {
    let idx = chunk.add_str(s);
    chunk.emit_rc(OpCode::LoadConst, d, idx, line);
}

pub(super) fn emit_const_char(chunk: &mut Chunk, d: u8, c: char, line: u32) {
    let idx = chunk.add_constant(PoolEntry::Literal(Literal::Char(c)));
    chunk.emit_rc(OpCode::LoadConst, d, idx, line);
}

pub(super) fn emit_const_decimal(
    chunk: &mut Chunk,
    d: u8,
    dec: &bigdecimal::BigDecimal,
    line: u32,
) {
    let idx = chunk.add_constant(PoolEntry::Literal(Literal::Decimal(dec.clone())));
    chunk.emit_rc(OpCode::LoadConst, d, idx, line);
}

pub(super) fn emit_const_bigint(chunk: &mut Chunk, d: u8, n: &Arc<str>, line: u32) -> Result<()> {
    let value: num_bigint::BigInt = n
        .parse()
        .map_err(|_| OptError::Unsupported("malformed bigint literal"))?;
    let idx = chunk.add_constant(PoolEntry::Literal(Literal::BigInt(value)));
    chunk.emit_rc(OpCode::LoadConst, d, idx, line);
    Ok(())
}

pub(super) fn emit_const_null(chunk: &mut Chunk, d: u8, line: u32) {
    chunk.emit_rr(OpCode::LoadNull, d, 0, line);
}

pub(super) fn emit_binary(
    chunk: &mut Chunk,
    d: u8,
    op: HirBinOp,
    lhs: Value,
    rhs: Value,
    ty: HirType,
    value_tys: &[HirType],
    reg: &[u8],
    line: u32,
) {
    let opcode = binary_opcode(
        op,
        ty,
        value_tys.get(lhs.0 as usize).copied(),
        value_tys.get(rhs.0 as usize).copied(),
    );
    chunk.emit_rrr(opcode, d, reg[lhs.0 as usize], reg[rhs.0 as usize], line);
}

pub(super) fn emit_unary(
    chunk: &mut Chunk,
    d: u8,
    op: HirUnOp,
    operand: Value,
    reg: &[u8],
    scratch: u8,
    line: u32,
) {
    let s = reg[operand.0 as usize];
    match op {
        HirUnOp::Neg => chunk.emit_rr(OpCode::Negate, d, s, line),
        HirUnOp::Not => chunk.emit_rr(OpCode::Not, d, s, line),
        HirUnOp::Typeof => chunk.emit_rr(OpCode::Typeof, d, s, line),
        HirUnOp::BitNot => {
            let idx = chunk.add_constant(PoolEntry::Literal(Literal::Int(-1)));
            chunk.emit_rc(OpCode::LoadConst, scratch, idx, line);
            chunk.emit_rrr(OpCode::BitXor, d, s, scratch, line);
        }
    }
}

pub(super) fn emit_is_null(chunk: &mut Chunk, d: u8, operand: Value, reg: &[u8], line: u32) {
    chunk.emit_rr(OpCode::IsNull, d, reg[operand.0 as usize], line);
}

pub(super) fn emit_cast(chunk: &mut Chunk, d: u8, operand: Value, reg: &[u8], line: u32) {
    let src = reg[operand.0 as usize];
    if d != src {
        chunk.emit_rr(OpCode::Move, d, src, line);
    }
}

pub(super) fn emit_convert(
    chunk: &mut Chunk,
    d: u8,
    operand: Value,
    reg: &[u8],
    conv: varn_core::NumConv,
    line: u32,
) {
    chunk.write(Chunk::pack_op(OpCode::Convert, d), line);
    chunk.write(Chunk::pack(reg[operand.0 as usize], conv as u8), line);
}

pub(super) fn emit_to_string(chunk: &mut Chunk, d: u8, operand: Value, reg: &[u8], line: u32) {
    chunk.emit_rr(OpCode::ToString, d, reg[operand.0 as usize], line);
}

pub(super) fn emit_get_enum_tag(chunk: &mut Chunk, d: u8, operand: Value, reg: &[u8], line: u32) {
    chunk.emit_rr(OpCode::GetEnumTag, d, reg[operand.0 as usize], line);
}

pub(super) fn emit_is_array(chunk: &mut Chunk, d: u8, operand: Value, reg: &[u8], line: u32) {
    chunk.emit_rr(OpCode::IsArray, d, reg[operand.0 as usize], line);
}

pub(super) fn emit_str_length(chunk: &mut Chunk, d: u8, operand: Value, reg: &[u8], line: u32) {
    chunk.emit_rr(OpCode::StrLength, d, reg[operand.0 as usize], line);
}

pub(super) fn emit_array_length(chunk: &mut Chunk, d: u8, operand: Value, reg: &[u8], line: u32) {
    chunk.emit_rr(OpCode::ArrayLength, d, reg[operand.0 as usize], line);
}

pub(super) fn emit_bytes_length(chunk: &mut Chunk, d: u8, operand: Value, reg: &[u8], line: u32) {
    chunk.emit_rr(OpCode::BytesLength, d, reg[operand.0 as usize], line);
}

pub(super) fn emit_object_keys(chunk: &mut Chunk, d: u8, operand: Value, reg: &[u8], line: u32) {
    chunk.emit_rr(OpCode::ObjectKeys, d, reg[operand.0 as usize], line);
}

pub(super) fn emit_get_symbol(
    chunk: &mut Chunk,
    d: u8,
    object: Value,
    reg: &[u8],
    is_async: bool,
    line: u32,
) {
    let sym = if is_async {
        RuntimeSymbol::AsyncIterator
    } else {
        RuntimeSymbol::Iterator
    };
    let idx = chunk.add_symbol(sym);
    chunk.emit_rrc(OpCode::GetSymbol, d, reg[object.0 as usize], idx, line);
}

pub(super) fn emit_this(chunk: &mut Chunk, d: u8, line: u32) {
    chunk.emit_rr(OpCode::Move, d, 0, line);
}
