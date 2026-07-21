//! Global-slot access lowering for CLIF: `LoadGlobalIdx` / `StoreGlobalIdx`.
//! Globals live in `ExecCtx.globals` (a non-`repr(C)` `GlobalStore`), whose
//! `values` Vec data pointer sits at `globals_offset + 8` — the same offset
//! the template loads. Globals are always GC roots, so a store needs no write
//! barrier. Split out of `lower.rs` for the file-size governance limit.

use cranelift_codegen::ir::{types, InstBuilder, MemFlags};
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_types::register_meta::RegisterMeta;

use super::emit::{box_int, state_meta_int};
use super::kinds::K;
use crate::JitHelpers;

/// Shared context for the global-access arms.
pub(super) struct GblCtx<'a> {
    pub vars: &'a [Variable],
    pub helpers: &'a JitHelpers,
    pub exec_ctx: cranelift_codegen::ir::Value,
    pub register_meta: &'a [RegisterMeta],
}

fn globals_base(b: &mut FunctionBuilder, c: &GblCtx) -> cranelift_codegen::ir::Value {
    b.ins().load(
        types::I64,
        MemFlags::trusted(),
        c.exec_ctx,
        (c.helpers.globals_offset + 8) as i32,
    )
}

/// `LoadGlobalIdx first_reg, idx` — load a global slot, unboxed to int when
/// the register meta proves it.
pub(super) fn emit_load_global_idx(
    b: &mut FunctionBuilder,
    c: &GblCtx,
    code: &[u16],
    ip: usize,
    first_reg: usize,
) {
    let idx = code[ip + 1] as usize;
    let gbase = globals_base(b, c);
    let v = b
        .ins()
        .load(types::I64, MemFlags::trusted(), gbase, (idx * 8) as i32);
    if state_meta_int(c.register_meta, first_reg) {
        let s = b.ins().ishl_imm(v, 16);
        let un = b.ins().sshr_imm(s, 16);
        b.def_var(c.vars[first_reg], un);
    } else {
        b.def_var(c.vars[first_reg], v);
    }
}

/// `StoreGlobalIdx src, idx` — plain boxed store, no barrier (globals are
/// roots). DefineGlobalIdx is NOT admitted: it can grow the globals vec and
/// move its base.
pub(super) fn emit_store_global_idx(
    b: &mut FunctionBuilder,
    c: &GblCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) -> Result<(), String> {
    let src = (code[ip + 1] >> 8) as usize;
    let idx = code[ip + 2] as usize;
    let v = match state[src] {
        K::Int => {
            let raw = b.use_var(c.vars[src]);
            box_int(b, raw)
        }
        K::Boxed => b.use_var(c.vars[src]),
        k => return Err(format!("clif: global store of {k:?}")),
    };
    let gbase = globals_base(b, c);
    b.ins()
        .store(MemFlags::trusted(), v, gbase, (idx * 8) as i32);
    Ok(())
}
