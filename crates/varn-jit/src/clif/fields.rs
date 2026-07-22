//! Object fixed-field access lowering for CLIF: `GetFixedField` /
//! `SetFixedField` (class-typed slot read/write). Both are plain slot
//! operations — no getter, no allocation, no GC, no closure — so they lower
//! to a bare helper call in either the alloc-free or the allocating path.
//! Split out of `lower.rs` for the file-size governance limit.

use cranelift_codegen::ir::{types, InstBuilder};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_types::register_meta::RegisterMeta;

use super::emit::{box_bool, box_int, call_helper, call_helper_void, state_meta_int, use_boxed};
use super::kinds::K;
use crate::JitHelpers;

/// Shared context for the fixed-field arms.
pub(super) struct FldCtx<'a> {
    pub vars: &'a [Variable],
    pub helpers: &'a JitHelpers,
    pub cc: CallConv,
    pub exec_ctx: cranelift_codegen::ir::Value,
    pub register_meta: &'a [RegisterMeta],
}

/// `GetFixedField first_reg, obj, slot` — plain slot read; result unboxed to
/// int when the register meta proves it.
pub(super) fn emit_get_fixed_field(
    b: &mut FunctionBuilder,
    c: &FldCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    first_reg: usize,
) -> Result<(), String> {
    let obj_r = (code[ip + 1] >> 8) as usize;
    let slot = code[ip + 2] as usize;
    let obj = use_boxed(b, c.vars, state, obj_r)?;
    let slot_v = b.ins().iconst(types::I64, slot as i64);
    let res = call_helper(b, c.cc, c.helpers.get_fixed_field, &[c.exec_ctx, obj, slot_v]);
    if state_meta_int(c.register_meta, first_reg) {
        let s = b.ins().ishl_imm(res, 16);
        let un = b.ins().sshr_imm(s, 16);
        b.def_var(c.vars[first_reg], un);
    } else {
        b.def_var(c.vars[first_reg], res);
    }
    Ok(())
}

/// `SetFixedField obj(=first_reg), val, slot` — slot write; the helper
/// carries the old←young write barrier.
pub(super) fn emit_set_fixed_field(
    b: &mut FunctionBuilder,
    c: &FldCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    first_reg: usize,
) -> Result<(), String> {
    let val_r = (code[ip + 1] >> 8) as usize;
    let slot = code[ip + 2] as usize;
    let obj = use_boxed(b, c.vars, state, first_reg)?;
    let val = match state[val_r] {
        K::Int => {
            let raw = b.use_var(c.vars[val_r]);
            box_int(b, raw)
        }
        K::Bool => {
            let raw = b.use_var(c.vars[val_r]);
            box_bool(b, raw)
        }
        _ => use_boxed(b, c.vars, state, val_r)?,
    };
    let slot_v = b.ins().iconst(types::I64, slot as i64);
    call_helper_void(
        b,
        c.cc,
        c.helpers.set_fixed_field,
        &[c.exec_ctx, obj, slot_v, val],
    );
    Ok(())
}
