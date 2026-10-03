use crate::hir::HirType;
use crate::ssa::ir::Value;
use crate::OptError;
use std::sync::Arc;
use varn_core::OpCode;
use varn_types::chunk::Chunk;

type Result<T> = std::result::Result<T, OptError>;

pub(super) fn emit_call(
    chunk: &mut Chunk,
    d: u8,
    callee: Value,
    args: &[Value],
    reg: &[u8],
    call_base: u8,
    line: u32,
) {
    super::super::terminator::emit_call_args(chunk, reg, call_base, args, line);
    let total = (args.len() + 1) as u8;
    chunk.emit(OpCode::Call, line);
    chunk.write(Chunk::pack(d, reg[callee.0 as usize]), line);
    chunk.write(Chunk::pack(total, call_base), line);
}

pub(super) fn emit_self_call(
    chunk: &mut Chunk,
    d: u8,
    args: &[Value],
    reg: &[u8],
    call_base: u8,
    line: u32,
) {
    super::super::terminator::emit_call_args(chunk, reg, call_base, args, line);
    let total = (args.len() + 1) as u8;
    chunk.emit(OpCode::CallSelf, line);
    chunk.write(Chunk::pack(d, 0), line);
    chunk.write(Chunk::pack(total, call_base), line);
}

pub(super) fn emit_method_call(
    chunk: &mut Chunk,
    d: u8,
    recv: Value,
    name: &Arc<str>,
    args: &[Value],
    value_tys: &[HirType],
    reg: &[u8],
    call_base: u8,
    ic_slot: Option<u8>,
    line: u32,
) -> Result<()> {
    let name_idx = chunk.add_str(name);
    for (i, a) in args.iter().enumerate() {
        chunk.emit_rr(OpCode::Move, call_base + i as u8, reg[a.0 as usize], line);
    }
    let argc = args.len() as u8;
    let cs = ic_slot.ok_or(OptError::Unsupported("ssa-emit: cache site without a slot"))?;
    let op = if matches!(
        value_tys.get(recv.0 as usize),
        Some(crate::hir::HirType::Class(_))
    ) {
        OpCode::InvokeVirtual
    } else {
        OpCode::CallMethod
    };
    chunk.write(Chunk::pack_op(op, cs), line);
    chunk.write(Chunk::pack(d, reg[recv.0 as usize]), line);
    chunk.write(name_idx, line);
    chunk.write(Chunk::pack(argc, call_base), line);
    Ok(())
}

pub(super) fn emit_iter_call(
    chunk: &mut Chunk,
    d: u8,
    callee: Value,
    recv: Value,
    reg: &[u8],
    call_base: u8,
    line: u32,
) {
    chunk.emit_rr(OpCode::Move, call_base, reg[recv.0 as usize], line);
    chunk.emit(OpCode::Call, line);
    chunk.write(Chunk::pack(d, reg[callee.0 as usize]), line);
    chunk.write(Chunk::pack(1, call_base), line);
}

pub(super) fn emit_super_call(
    chunk: &mut Chunk,
    d: u8,
    args: &[Value],
    reg: &[u8],
    call_base: u8,
    line: u32,
) {
    let ctor_idx = chunk.add_str("constructor");
    chunk.emit_rc(OpCode::GetSuper, call_base, ctor_idx, line);
    chunk.emit_rr(OpCode::Move, call_base + 1, 0, line);
    for (i, a) in args.iter().enumerate() {
        chunk.emit_rr(
            OpCode::Move,
            call_base + 2 + i as u8,
            reg[a.0 as usize],
            line,
        );
    }
    let total = (args.len() + 1) as u8;
    chunk.emit(OpCode::Call, line);
    chunk.write(Chunk::pack(call_base, call_base), line);
    chunk.write(Chunk::pack(total, call_base + 1), line);
    chunk.emit_rr(OpCode::Move, d, call_base + 1, line);
}

pub(super) fn emit_super_method_call(
    chunk: &mut Chunk,
    d: u8,
    name: &Arc<str>,
    args: &[Value],
    reg: &[u8],
    call_base: u8,
    line: u32,
) {
    let name_idx = chunk.add_str(name);
    chunk.emit_rc(OpCode::GetSuper, call_base, name_idx, line);
    for (i, a) in args.iter().enumerate() {
        chunk.emit_rr(
            OpCode::Move,
            call_base + 1 + i as u8,
            reg[a.0 as usize],
            line,
        );
    }
    let count = args.len() as u8;
    chunk.emit(OpCode::Call, line);
    chunk.write(Chunk::pack(d, call_base), line);
    chunk.write(
        Chunk::pack(count, if count > 0 { call_base + 1 } else { 0 }),
        line,
    );
}

pub(super) fn emit_extension_call(
    chunk: &mut Chunk,
    d: u8,
    func: &Arc<str>,
    slot: Option<u32>,
    recv: Value,
    args: &[Value],
    reg: &[u8],
    call_base: u8,
    line: u32,
) -> Result<()> {
    match slot {
        Some(s) => {
            let s = u16::try_from(s).map_err(|_| {
                OptError::Unsupported("ssa-emit: extension global slot exceeds u16")
            })?;
            chunk.emit_rc(OpCode::LoadGlobalIdx, call_base, s, line);
        }
        None => {
            let idx = chunk.add_str(func);
            chunk.emit_rc(OpCode::LoadGlobal, call_base, idx, line);
        }
    }
    chunk.emit_rr(OpCode::Move, call_base + 1, reg[recv.0 as usize], line);
    for (i, a) in args.iter().enumerate() {
        chunk.emit_rr(
            OpCode::Move,
            call_base + 2 + i as u8,
            reg[a.0 as usize],
            line,
        );
    }
    let total = (args.len() + 1) as u8;
    chunk.emit(OpCode::Call, line);
    chunk.write(Chunk::pack(d, call_base), line);
    chunk.write(Chunk::pack(total, call_base + 1), line);
    Ok(())
}

pub(super) fn emit_call_spread(
    chunk: &mut Chunk,
    d: u8,
    callee: Value,
    args: &[(Value, bool)],
    reg: &[u8],
    call_base: u8,
    line: u32,
) {
    chunk.emit_rr(OpCode::LoadNull, call_base, 0, line);
    for (i, (a, spread)) in args.iter().enumerate() {
        let op = if *spread {
            OpCode::WrapSpread
        } else {
            OpCode::Move
        };
        chunk.emit_rr(op, call_base + 1 + i as u8, reg[a.0 as usize], line);
    }
    let total = (args.len() + 1) as u8;
    chunk.emit(OpCode::CallSpread, line);
    chunk.write(Chunk::pack(d, reg[callee.0 as usize]), line);
    chunk.write(Chunk::pack(total, call_base), line);
}

pub(super) fn emit_intrinsic_direct(
    chunk: &mut Chunk,
    d: u8,
    args: &[Value],
    wire_byte: u8,
    reg: &[u8],
    line: u32,
) {
    chunk.write(Chunk::pack_op(OpCode::IntrinsicDirect, d), line);
    chunk.write(
        ((reg[args[0].0 as usize] as u16) << 8) | wire_byte as u16,
        line,
    );
}

pub(super) fn emit_intrinsic(
    chunk: &mut Chunk,
    d: u8,
    object: Value,
    args: &[Value],
    wire_byte: u8,
    reg: &[u8],
    call_base: u8,
    line: u32,
) {
    chunk.emit_rr(OpCode::Move, call_base, reg[object.0 as usize], line);
    for (i, a) in args.iter().enumerate() {
        chunk.emit_rr(
            OpCode::Move,
            call_base + 1 + i as u8,
            reg[a.0 as usize],
            line,
        );
    }
    let arg_count = (args.len() + 1) as u16;
    chunk.write(Chunk::pack_op(OpCode::Intrinsic, call_base), line);
    chunk.write(((wire_byte as u16) << 8) | arg_count, line);
    chunk.emit_rr(OpCode::Move, d, call_base, line);
}

pub(super) fn emit_call_native_op(
    chunk: &mut Chunk,
    d: u8,
    object: Value,
    args: &[Value],
    op_id: u64,
    reg: &[u8],
    call_base: u8,
    line: u32,
) {
    chunk.emit_rr(OpCode::Move, call_base, reg[object.0 as usize], line);
    for (i, a) in args.iter().enumerate() {
        chunk.emit_rr(
            OpCode::Move,
            call_base + 1 + i as u8,
            reg[a.0 as usize],
            line,
        );
    }
    let arg_count = (args.len() + 1) as u16;
    let cidx = chunk.add_int(op_id as i64);
    chunk.write(Chunk::pack_op(OpCode::CallNativeOp, call_base), line);
    chunk.write(cidx, line);
    chunk.write(arg_count, line);
    chunk.emit_rr(OpCode::Move, d, call_base, line);
}
