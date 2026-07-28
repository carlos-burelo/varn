//! Method call lowering for CLIF: `CallMethod` and `InvokeVirtual`.
//! Handled via fast-path flat dispatch helpers that manage VM frames,
//! GC safepoints, and register reloads.

use cranelift_codegen::ir::{types, InstBuilder};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::{RegisterMeta, SlotKind};

use super::alloc::{flush_boxed, frame_base_addr, live_boxed, reload_boxed, store_home, AllocCtx};
use super::emit::{box_or_pass, call_helper, meta_is_float, unbox_f64_coerce};
use super::kinds::K;

/// `CallMethod` lowering:
/// w0: `[OpCode, cs]`
/// w1: `[dest, this_reg]`
/// w2: `name_idx`
/// w3: `[argc, arg_start]`
pub(super) fn emit_call_method(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    meta: &[RegisterMeta],
    code: &[u16],
    ip: usize,
) {
    let cs = (code[ip] & 0xFF) as usize;
    let dest = (code[ip + 1] >> 8) as usize;
    let this_reg = (code[ip + 1] & 0xFF) as usize;
    let name_idx = code[ip + 2] as usize;
    let argc = (code[ip + 3] >> 8) as usize;
    let arg_start = (code[ip + 3] & 0xFF) as usize;
    let next_ip = ip + 4;

    let fb = frame_base_addr(b, actx);
    for i in 0..argc {
        store_home(b, actx, state, fb, arg_start + i);
    }
    let this_val = box_or_pass(b, actx.vars, state, this_reg);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let ni = b.ins().iconst(types::I64, name_idx as i64);
    let ci = b.ins().iconst(types::I64, cs as i64);
    let ast = b.ins().iconst(types::I64, arg_start as i64);
    let ac = b.ins().iconst(types::I64, argc as i64);
    let de = b.ins().iconst(types::I64, dest as i64);
    let ipv = b.ins().iconst(types::I64, next_ip as i64);

    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.call_method_flat,
        &[actx.exec_ctx, actx.closure, this_val, ni, ci, ast, ac, de, ipv],
    );

    reload_boxed(b, actx, &regs);

    if meta_is_float(meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(actx.vars[dest], f);
    } else if meta.get(dest).map_or(false, |m| m.kind == SlotKind::Int) {
        let s = b.ins().ishl_imm(res, 16);
        let un = b.ins().sshr_imm(s, 16);
        b.def_var(actx.vars[dest], un);
    } else {
        b.def_var(actx.vars[dest], res);
    }
}

/// `InvokeVirtual` lowering:
/// w1: `[dest, this_reg]`
/// w2: `name_idx`
/// w3: `[argc, arg_start]`
pub(super) fn emit_invoke_virtual(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    meta: &[RegisterMeta],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip + 1] >> 8) as usize;
    let this_reg = (code[ip + 1] & 0xFF) as usize;
    let name_idx = code[ip + 2] as usize;
    let argc = (code[ip + 3] >> 8) as usize;
    let arg_start = (code[ip + 3] & 0xFF) as usize;
    let next_ip = ip + 4;

    let fb = frame_base_addr(b, actx);
    for i in 0..argc {
        store_home(b, actx, state, fb, arg_start + i);
    }
    let this_val = box_or_pass(b, actx.vars, state, this_reg);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let ni = b.ins().iconst(types::I64, name_idx as i64);
    let ast = b.ins().iconst(types::I64, arg_start as i64);
    let ac = b.ins().iconst(types::I64, argc as i64);
    let de = b.ins().iconst(types::I64, dest as i64);
    let ipv = b.ins().iconst(types::I64, next_ip as i64);

    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.invoke_virtual_flat,
        &[actx.exec_ctx, actx.closure, this_val, ni, ast, ac, de, ipv],
    );

    reload_boxed(b, actx, &regs);

    if meta_is_float(meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(actx.vars[dest], f);
    } else if meta.get(dest).map_or(false, |m| m.kind == SlotKind::Int) {
        let s = b.ins().ishl_imm(res, 16);
        let un = b.ins().sshr_imm(s, 16);
        b.def_var(actx.vars[dest], un);
    } else {
        b.def_var(actx.vars[dest], res);
    }
}
