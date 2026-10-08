use crate::hir::HirUpvalueSrc;
use crate::ssa::ir::{BlockId, Value, VarId};
use crate::OptError;
use std::rc::Rc;
use std::sync::Arc;
use varn_core::OpCode;
use varn_types::chunk::{Chunk, PoolEntry};

type Result<T> = std::result::Result<T, OptError>;

pub(super) fn emit_make_closure(
    chunk: &mut Chunk,
    d: u8,
    func: u32,
    upvalues_src: &[HirUpvalueSrc],
    source_file: &Arc<str>,
    nparams: usize,
    closure_const: &mut Option<u16>,
    line: u32,
    scope: &crate::from_tir::compile::ModuleScope,
) -> Result<()> {
    let proto = crate::from_tir::compile::compile_closure(scope, func, source_file.clone())?;
    let idx = chunk.add_constant(PoolEntry::Function(Rc::new(proto)));
    *closure_const = Some(idx);
    if upvalues_src.is_empty() {
        chunk.write(Chunk::pack_op(OpCode::LoadStaticFn, d), line);
        chunk.write(idx, line);
    } else {
        let uv_count = upvalues_src.len() as u8;
        chunk.emit(OpCode::MakeClosure, line);
        chunk.write(Chunk::pack(d, uv_count), line);
        chunk.write(idx, line);
        for uv_src in upvalues_src {
            let is_local = match uv_src {
                HirUpvalueSrc::ParentLocal(_) | HirUpvalueSrc::ParentParam(_) => 1u8,
                HirUpvalueSrc::ParentUpvalue(_) => 0u8,
            };
            let index = match uv_src {
                HirUpvalueSrc::ParentLocal(id) => {
                    super::super::regs::var_reg(VarId::Local(*id), nparams)
                }
                HirUpvalueSrc::ParentParam(i) => {
                    super::super::regs::var_reg(VarId::Param(*i), nparams)
                }
                HirUpvalueSrc::ParentUpvalue(idx) => *idx as u8,
            };
            chunk.write(Chunk::pack(is_local, index), line);
        }
    }
    Ok(())
}

pub(super) fn emit_make_class(
    chunk: &mut Chunk,
    d: u8,
    name: &Arc<str>,
    super_class: Option<Value>,
    reg: &[u8],
    line: u32,
) {
    let name_idx = chunk.add_str(name);
    let super_reg = super_class.map(|sc| reg[sc.0 as usize]).unwrap_or(0);
    chunk.emit_rrc(OpCode::MakeClass, d, super_reg, name_idx, line);
}

pub(super) fn emit_make_enum_variant(
    chunk: &mut Chunk,
    d: u8,
    tag: i64,
    meta: &Arc<str>,
    scratch: u8,
    line: u32,
) {
    let meta_idx = chunk.add_str(meta);
    chunk.emit_load_int(scratch, tag, line);
    chunk.emit(OpCode::MakeEnumVariant, line);
    chunk.write(Chunk::pack(d, scratch), line);
    chunk.write(meta_idx, line);
}

pub(super) fn emit_try(
    chunk: &mut Chunk,
    d: u8,
    handler: BlockId,
    fixups: &mut Vec<(usize, BlockId)>,
    line: u32,
) {
    chunk.emit(OpCode::Try, line);
    chunk.write(Chunk::pack(d, 0), line);
    let pos = chunk.code.len();
    chunk.write(0xFFFF, line);
    chunk.write(0xFFFF, line);
    fixups.push((pos, handler));
}

pub(super) fn emit_catch_param(chunk: &mut Chunk, d: u8, try_val: Value, reg: &[u8], line: u32) {
    chunk.emit_rr(OpCode::Move, d, reg[try_val.0 as usize], line);
}

pub(super) fn emit_await(chunk: &mut Chunk, d: u8, operand: Value, reg: &[u8], line: u32) {
    chunk.emit_rr(OpCode::Await, d, reg[operand.0 as usize], line);
}

pub(super) fn emit_spawn(chunk: &mut Chunk, d: u8, operand: Value, reg: &[u8], line: u32) {
    chunk.emit_rr(OpCode::Spawn, d, reg[operand.0 as usize], line);
}

pub(super) fn emit_yield(chunk: &mut Chunk, d: u8, operand: Value, reg: &[u8], line: u32) {
    chunk.emit1(OpCode::Yield, Chunk::pack(d, reg[operand.0 as usize]), line);
}
