//! Instructions emitted for their effect: stores, property writes, class
//! member definitions, scope teardown.
//!
//! Returns whether the instruction was handled here. Every arm of the match
//! terminates the instruction, so a `false` means "not an effect instruction"
//! and sends the caller on to `values`.

use super::super::ir::{Inst, InstKind, VarId};
use super::regs::var_reg;
use crate::OptError;
use varn_core::OpCode;
use varn_types::chunk::Chunk;

type Result<T> = std::result::Result<T, OptError>;

pub(super) fn emit_effect(
    chunk: &mut Chunk,
    inst: &Inst,
    reg: &[u8],
    cache_count: &mut u16,
    nparams: usize,
) -> Result<bool> {
    let line = inst.line;
    match &inst.kind {
        InstKind::SetProperty {
            object,
            name,
            value,
        } => {
            let idx = chunk.add_str(name);
            if *cache_count > 255 {
                return Err(OptError::Unsupported(
                    "ssa-emit: too many inline-cache sites",
                ));
            }
            let cs = *cache_count as u8;
            *cache_count += 1;
            chunk.emit_rrc_ic(
                OpCode::SetProperty,
                reg[object.0 as usize],
                reg[value.0 as usize],
                idx,
                cs,
                line,
            );
            return Ok(true);
        }
        InstKind::SetFixedField {
            object,
            value,
            slot,
        } => {
            chunk.emit_rrc(
                OpCode::SetFixedField,
                reg[object.0 as usize],
                reg[value.0 as usize],
                *slot,
                line,
            );
            return Ok(true);
        }
        InstKind::SetIndex {
            object,
            index,
            value,
        } => {
            chunk.emit_rrr(
                OpCode::SetIndex,
                reg[object.0 as usize],
                reg[index.0 as usize],
                reg[value.0 as usize],
                line,
            );
            return Ok(true);
        }
        InstKind::ArraySetIndex {
            object,
            index,
            value,
        } => {
            chunk.emit_rrr(
                OpCode::ArraySetIndex,
                reg[object.0 as usize],
                reg[index.0 as usize],
                reg[value.0 as usize],
                line,
            );
            return Ok(true);
        }
        InstKind::ArrayPush { array, value } => {
            chunk.emit_rr(
                OpCode::ArrayPush,
                reg[array.0 as usize],
                reg[value.0 as usize],
                line,
            );
            return Ok(true);
        }
        InstKind::ObjectMerge { target, source } => {
            chunk.emit_rr(
                OpCode::ObjectMerge,
                reg[target.0 as usize],
                reg[source.0 as usize],
                line,
            );
            return Ok(true);
        }

        InstKind::AssertNotNull { operand } => {
            chunk.emit1(
                OpCode::AssertNotNull,
                Chunk::pack(reg[operand.0 as usize], 0),
                line,
            );
            return Ok(true);
        }

        InstKind::StoreGlobal { name, value } => {
            let idx = chunk.add_str(name);
            chunk.emit_rrc(OpCode::DefineGlobal, 0, reg[value.0 as usize], idx, line);
            return Ok(true);
        }

        InstKind::StoreUpvalue { index, value } => {
            chunk.emit1(
                OpCode::StoreUpvalue,
                Chunk::pack(*index as u8, reg[value.0 as usize]),
                line,
            );
            return Ok(true);
        }
        InstKind::StoreCaptured { var, value } => {
            let dest_reg = var_reg(*var, nparams);
            chunk.emit_rr(OpCode::Move, dest_reg, reg[value.0 as usize], line);
            return Ok(true);
        }
        InstKind::StoreModuleSlot { value, slot } => {
            chunk.emit_rc(OpCode::StoreModuleSlot, reg[value.0 as usize], *slot, line);
            return Ok(true);
        }
        InstKind::CloseUpvalues { targets } => {
            let lowest = targets
                .iter()
                .map(|t| var_reg(*t, nparams))
                .min()
                .unwrap_or(0);
            chunk.emit1(OpCode::CloseUpvalue, lowest as u16, line);
            return Ok(true);
        }
        InstKind::Dispose { target, is_await } => {
            let r = var_reg(VarId::Local(*target), nparams);
            let method = if *is_await { "disposeAsync" } else { "dispose" };
            let str_idx = chunk.add_str(method);
            if *cache_count > 255 {
                return Err(OptError::Unsupported(
                    "ssa-emit: too many inline-cache sites",
                ));
            }
            let cs = *cache_count as u8;
            *cache_count += 1;
            chunk.write(Chunk::pack_op(OpCode::CallMethod, cs), line);
            chunk.write(Chunk::pack(r, r), line);
            chunk.write(str_idx, line);
            chunk.write(Chunk::pack(0u8, 0u8), line);
            return Ok(true);
        }
        InstKind::PopTry => {
            chunk.emit(OpCode::PopTry, line);
            return Ok(true);
        }
        InstKind::DeclareField { class, name } => {
            let class_reg = reg[class.0 as usize];
            let key_idx = chunk.add_str(name);
            chunk.emit(OpCode::DeclareField, line);
            chunk.write(Chunk::pack(class_reg, 0), line);
            chunk.write(key_idx, line);
            return Ok(true);
        }
        InstKind::DefineStatic { class, name, value } => {
            let class_reg = reg[class.0 as usize];
            let val_reg = reg[value.0 as usize];
            let key_idx = chunk.add_str(name);
            chunk.emit(OpCode::DefineStatic, line);
            chunk.write(Chunk::pack(class_reg, val_reg), line);
            chunk.write(key_idx, line);
            return Ok(true);
        }
        InstKind::DefineMethod {
            class,
            name,
            method,
            is_static,
        } => {
            let class_reg = reg[class.0 as usize];
            let method_reg = reg[method.0 as usize];
            let key_idx = chunk.add_str(name);
            let op = if *is_static {
                OpCode::DefineStatic
            } else {
                OpCode::Method
            };
            chunk.emit(op, line);
            chunk.write(Chunk::pack(class_reg, method_reg), line);
            chunk.write(key_idx, line);
            return Ok(true);
        }
        InstKind::DefineAccessor {
            class,
            name,
            accessor,
            is_getter,
            is_static,
        } => {
            let class_reg = reg[class.0 as usize];
            let acc_reg = reg[accessor.0 as usize];
            let key_idx = chunk.add_str(name);
            let op = match (is_getter, is_static) {
                (true, true) => OpCode::DefineStaticGetter,
                (true, false) => OpCode::DefineGetter,
                (false, true) => OpCode::DefineStaticSetter,
                (false, false) => OpCode::DefineSetter,
            };
            chunk.emit(op, line);
            chunk.write(Chunk::pack(class_reg, acc_reg), line);
            chunk.write(key_idx, line);
            return Ok(true);
        }
        _ => {}
    }

    Ok(false)
}
