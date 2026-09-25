//! Fixed-field access lowering for CLIF: `GetFixedField` / `SetFixedField`.
//!
//! A field is addressed one of two ways (`varn_core::FieldAccess`): a class
//! field at the compact offset the compiler baked ([`compact`], shared with
//! the lowering from typed SSA), or an object/record/enum-payload field by
//! dynamic slot ([`slot`], reads only — a write is always to a class field).

use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_types::register_meta::RegisterMeta;

use super::alloc::AllocCtx;
use super::emit;
use super::kinds::K;
use crate::JitHelpers;

mod compact;
mod slot;

pub(crate) use compact::{load_compact, store_compact, FieldIo};

/// Shared context for the fixed-field arms.
pub(crate) struct FldCtx<'a> {
    pub vars: &'a [Variable],
    pub helpers: &'a JitHelpers,
    pub cc: CallConv,
    pub exec_ctx: cranelift_codegen::ir::Value,
    pub register_meta: &'a [RegisterMeta],
    pub loop_caches: emit::LoopCaches<'a>,
    pub local_obj_bases: &'a rustc_hash::FxHashMap<usize, Variable>,
}

/// `GetFixedField first_reg, obj|access, slot[, offset]`.
pub(super) fn emit_get_fixed_field(
    b: &mut FunctionBuilder,
    c: &FldCtx,
    actx: Option<&AllocCtx>,
    state: &[K],
    code: &[u16],
    ip: usize,
    first_reg: usize,
) -> Result<(), String> {
    let obj_r = (code[ip + 1] >> 8) as usize;
    let slot = code[ip + 2] as usize;
    match varn_core::FieldAccess::decode((code[ip + 1] & 0xFF) as u8) {
        varn_core::FieldAccess::Compact(kind) => compact::emit_get(
            b,
            c,
            actx,
            state,
            first_reg,
            obj_r,
            code[ip + 3] as u32,
            kind,
            slot,
        ),
        varn_core::FieldAccess::Slot => slot::emit_get(b, c, actx, state, ip, first_reg, obj_r, slot),
    }
}

/// `SetFixedField obj(=first_reg), val|access, slot, offset` — always a
/// compact class field.
pub(super) fn emit_set_fixed_field(
    b: &mut FunctionBuilder,
    c: &FldCtx,
    actx: Option<&AllocCtx>,
    state: &[K],
    code: &[u16],
    ip: usize,
    first_reg: usize,
) -> Result<(), String> {
    let val_r = (code[ip + 1] >> 8) as usize;
    let slot = code[ip + 2] as usize;
    match varn_core::FieldAccess::decode((code[ip + 1] & 0xFF) as u8) {
        varn_core::FieldAccess::Compact(kind) => compact::emit_set(
            b,
            c,
            actx,
            state,
            first_reg,
            val_r,
            code[ip + 3] as u32,
            kind,
            slot,
        ),
        varn_core::FieldAccess::Slot => {
            Err("clif: SetFixedField by slot is never emitted (a write is to a class field)".into())
        }
    }
}
