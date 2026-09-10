//! Global-slot access lowering for CLIF: `LoadGlobalIdx` (module-relative),
//! `LoadNativeGlobalIdx` (absolute prelude region) and `StoreGlobalIdx`.
//! Globals live in `ExecCtx.globals` (a non-`repr(C)` `GlobalStore`), whose
//! `values` Vec data pointer sits at `globals_offset + 8`. Globals are always
//! GC roots, so a store needs no write barrier. Split out of `lower.rs` for the
//! file-size governance limit.
//!
//! `LoadGlobalIdx` / `StoreGlobalIdx` carry a slot RELATIVE to the running
//! closure's module region; the lowering loads `module_base` from the closure
//! param (`closure_module_base_offset`) and adds it. `LoadNativeGlobalIdx` is
//! absolute. The compiler emits these directly (the checker numbers module
//! globals, `varn_builtins::native_global_layout` numbers the prelude), so a
//! name-keyed `LoadGlobal` reaching here means a genuinely dynamic name — it
//! bails, and the function drops to the interpreter. Diagnose with
//! `VARN_CLIF_TRACE=1 VARN_JIT_TIER=1`.

use cranelift_codegen::ir::{types, InstBuilder, MemFlags};
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_types::register_meta::RegisterMeta;

use super::alloc::{box_or_load_home, def_result, AllocCtx};
use super::emit::{box_or_pass, meta_is_float, state_meta_int, unbox_f64_coerce, unbox_int};
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

/// Byte address of global slot `idx`. When `relative`, `idx` is added to the
/// running closure's `module_base` (loaded from the closure param); otherwise
/// it is absolute (the native/prelude region).
///
/// A relative access with no `actx` (a leaf lowering, no closure param) falls
/// back to absolute — but such a lowering also touches `c.exec_ctx`, the dummy
/// that forces the frame-aware retry, so this address is never actually run.
fn slot_addr(
    b: &mut FunctionBuilder,
    c: &GblCtx,
    idx: usize,
    relative: bool,
) -> cranelift_codegen::ir::Value {
    let gbase = globals_base(b, c);
    let eff = match (relative, c.actx) {
        (true, Some(actx)) => {
            let mb = b.ins().load(
                types::I32,
                MemFlags::trusted(),
                actx.closure,
                c.helpers.closure_module_base_offset as i32,
            );
            let mb = b.ins().uextend(types::I64, mb);
            let idx_v = b.ins().iconst(types::I64, idx as i64);
            b.ins().iadd(mb, idx_v)
        }
        _ => b.ins().iconst(types::I64, idx as i64),
    };
    let scaled = b.ins().imul_imm(eff, 16);
    b.ins().iadd(gbase, scaled)
}

fn store_load_result(
    b: &mut FunctionBuilder,
    c: &GblCtx,
    first_reg: usize,
    v: cranelift_codegen::ir::Value,
) {
    if let Some(actx) = c.actx {
        def_result(b, actx, first_reg, v);
    } else if meta_is_float(c.register_meta, first_reg) {
        let f = unbox_f64_coerce(b, v);
        b.def_var(c.vars[first_reg], f);
    } else if state_meta_int(c.register_meta, first_reg) {
        let un = unbox_int(b, v);
        b.def_var(c.vars[first_reg], un);
    } else {
        let (_tag, payload) = b.ins().isplit(v);
        b.def_var(c.vars[first_reg], payload);
    }
}

/// `LoadGlobalIdx first_reg, idx` — module-relative global read.
pub(super) fn emit_load_global_idx(
    b: &mut FunctionBuilder,
    c: &GblCtx,
    code: &[u16],
    ip: usize,
    first_reg: usize,
) {
    let idx = code[ip + 1] as usize;
    let addr = slot_addr(b, c, idx, true);
    let v = b.ins().load(types::I128, MemFlags::trusted(), addr, 0);
    store_load_result(b, c, first_reg, v);
}

/// `LoadNativeGlobalIdx first_reg, idx` — absolute (native/prelude) global read.
pub(super) fn emit_load_native_global_idx(
    b: &mut FunctionBuilder,
    c: &GblCtx,
    code: &[u16],
    ip: usize,
    first_reg: usize,
) {
    let idx = code[ip + 1] as usize;
    let addr = slot_addr(b, c, idx, false);
    let v = b.ins().load(types::I128, MemFlags::trusted(), addr, 0);
    store_load_result(b, c, first_reg, v);
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
            let tag = b
                .ins()
                .iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64);
            b.ins().iconcat(tag, raw)
        }
    };
    let addr = slot_addr(b, c, idx, true);
    b.ins().store(MemFlags::trusted(), v, addr, 0);
    Ok(())
}
