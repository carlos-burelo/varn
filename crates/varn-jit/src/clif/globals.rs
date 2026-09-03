//! Global-slot access lowering for CLIF: `LoadGlobalIdx` / `StoreGlobalIdx`.
//! Globals live in `ExecCtx.globals` (a non-`repr(C)` `GlobalStore`), whose
//! `values` Vec data pointer sits at `globals_offset + 8`. Globals are always
//! GC roots, so a store needs no write barrier. Split out of `lower.rs` for the
//! file-size governance limit.
//!
//! The indexed forms are the ONLY ones lowered, and that is an invariant, not a
//! subset: `varn_vm::globals::resolve_in_proto` rewrites every name-keyed
//! Global-slot access lowering for CLIF: `LoadGlobalIdx` / `StoreGlobalIdx`.
//! Globals live in `ExecCtx.globals` (a non-`repr(C)` `GlobalStore`), whose
//! `values` Vec data pointer sits at `globals_offset + 8`. Globals are always
//! GC roots, so a store needs no write barrier. Split out of `lower.rs` for the
//! file-size governance limit.
//!
//! The indexed forms are the ONLY ones lowered, and that is an invariant, not a
//! subset: `varn_vm::globals::resolve_in_proto` rewrites every name-keyed
//! `LoadGlobal`/`StoreGlobal`/`DefineGlobal` before the proto can run, in every
//! VM. A name-keyed global reaching here is a bug in that pass, and it bails —
//! the function silently drops to the interpreter. If that ever needs
//! diagnosing, the cheap check is to make those three opcodes a distinct bail
//! message and run the suite with `VARN_CLIF_TRACE=1 VARN_JIT_TIER=1`.

use cranelift_codegen::ir::{types, InstBuilder, MemFlags};
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_types::register_meta::RegisterMeta;

use super::alloc::{box_or_load_home, def_result, AllocCtx};
use super::emit::{box_or_pass, meta_is_float, state_meta_int, unbox_f64_coerce, wrap_i48};
use super::kinds::K;
use crate::JitHelpers;

/// Shared context for the global-access arms.
pub(crate) struct GblCtx<'a> {
    pub vars: &'a [Variable],
    pub register_meta: &'a [RegisterMeta],
    pub exec_ctx: cranelift_codegen::ir::Value,
    pub helpers: &'a JitHelpers,
    pub actx: Option<&'a AllocCtx<'a>>,
}

fn globals_base(b: &mut FunctionBuilder, c: &GblCtx) -> cranelift_codegen::ir::Value {
    b.ins().load(
        types::I64,
        MemFlags::trusted(),
        c.exec_ctx,
        c.helpers.globals_offset as i32,
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
        .load(types::I128, MemFlags::trusted(), gbase, (idx * 16) as i32);
    if let Some(actx) = c.actx {
        def_result(b, actx, first_reg, v);
    } else if meta_is_float(c.register_meta, first_reg) {
        let f = unbox_f64_coerce(b, v);
        b.def_var(c.vars[first_reg], f);
    } else if state_meta_int(c.register_meta, first_reg) {
        let un = wrap_i48(b, v);
        b.def_var(c.vars[first_reg], un);
    } else {
        let (_tag, payload) = b.ins().isplit(v);
        b.def_var(c.vars[first_reg], payload);
    }
}

/// `StoreGlobalIdx src, idx` / `DefineGlobalIdx src, idx` — plain boxed store,
/// no barrier (globals are roots).
pub(super) fn emit_store_global_idx(
    b: &mut FunctionBuilder,
    c: &GblCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) -> Result<(), String> {
    let src = (code[ip + 1] >> 8) as usize;
    let idx = code[ip + 2] as usize;
    let v = if let Some(actx) = c.actx {
        box_or_load_home(b, actx, state, src)
    } else {
        let raw = box_or_pass(b, c.vars, state, src);
        if b.func.dfg.value_type(raw) == types::I128 {
            raw
        } else {
            let tag = b.ins().iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64);
            b.ins().iconcat(tag, raw)
        }
    };
    let gbase = globals_base(b, c);
    b.ins()
        .store(MemFlags::trusted(), v, gbase, (idx * 16) as i32);
    Ok(())
}
