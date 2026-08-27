//! Method call lowering for CLIF: `CallMethod` and `InvokeVirtual`.
//! Handled via fast-path flat dispatch helpers that manage VM frames,
//! GC safepoints, and register reloads.

use cranelift_codegen::ir::{types, InstBuilder};
use cranelift_frontend::FunctionBuilder;
use varn_types::FunctionProto;

use super::alloc::{flush_boxed, frame_base_addr, live_boxed, reload_boxed, store_home, AllocCtx};
use super::emit::{box_or_pass, call_helper, call_helper_void, meta_is_float, unbox_f64_coerce};
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
    proto: &FunctionProto,
    code: &[u16],
    ip: usize,
) {
    let cs = (code[ip] >> 8) as usize;
    let dest = (code[ip + 1] >> 8) as usize;
    let this_reg = (code[ip + 1] & 0xFF) as usize;
    let name_idx = code[ip + 2] as usize;
    let argc = (code[ip + 3] >> 8) as usize;
    let arg_start = (code[ip + 3] & 0xFF) as usize;
    let next_ip = ip + 4;

    // Fast path: known intrinsic Array push / pop methods
    if let Some(name) = proto.chunk.constants.get(name_idx).and_then(|p| p.as_str()) {
        if name == varn_core::MemberKey::Push.as_str() && argc == 1 {
            let this_val = box_or_pass(b, actx.vars, state, this_reg);
            let val = box_or_pass(b, actx.vars, state, arg_start);
            let regs = live_boxed(actx, state);
            flush_boxed(b, actx, state, &regs);
            call_helper_void(
                b,
                actx.cc,
                actx.helpers.array_push,
                &[actx.exec_ctx, this_val, val],
            );
            reload_boxed(b, actx, state, &regs);
            let null_v = b.ins().iconst(types::I64, 0);
            b.def_var(actx.vars[dest], null_v);
            return;
        } else if name == varn_core::MemberKey::Pop.as_str() && argc == 0 {
            let this_val = box_or_pass(b, actx.vars, state, this_reg);
            let regs = live_boxed(actx, state);
            flush_boxed(b, actx, state, &regs);
            let res = call_helper(
                b,
                actx.cc,
                actx.helpers.array_pop,
                &[actx.exec_ctx, this_val],
            );
            reload_boxed(b, actx, state, &regs);
            if meta_is_float(&proto.register_meta, dest) {
                let f = unbox_f64_coerce(b, res);
                b.def_var(actx.vars[dest], f);
            } else {
                b.def_var(actx.vars[dest], res);
            }
            return;
        }
    }

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
        &[
            actx.exec_ctx,
            actx.closure,
            actx.base,
            this_val,
            ni,
            ci,
            ast,
            ac,
            de,
            ipv,
        ],
    );

    reload_boxed(b, actx, state, &regs);

    if meta_is_float(&proto.register_meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(actx.vars[dest], f);
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
    proto: &FunctionProto,
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip + 1] >> 8) as usize;
    let this_reg = (code[ip + 1] & 0xFF) as usize;
    let name_idx = code[ip + 2] as usize;
    let argc = (code[ip + 3] >> 8) as usize;
    let arg_start = (code[ip + 3] & 0xFF) as usize;
    let next_ip = ip + 4;

    // Fast path: known intrinsic Array push / pop methods
    if let Some(name) = proto.chunk.constants.get(name_idx).and_then(|p| p.as_str()) {
        if name == varn_core::MemberKey::Push.as_str() && argc == 1 {
            let this_val = box_or_pass(b, actx.vars, state, this_reg);
            let val = box_or_pass(b, actx.vars, state, arg_start);
            let regs = live_boxed(actx, state);
            flush_boxed(b, actx, state, &regs);
            call_helper_void(
                b,
                actx.cc,
                actx.helpers.array_push,
                &[actx.exec_ctx, this_val, val],
            );
            reload_boxed(b, actx, state, &regs);
            let null_v = b.ins().iconst(types::I64, 0);
            b.def_var(actx.vars[dest], null_v);
            return;
        } else if name == varn_core::MemberKey::Pop.as_str() && argc == 0 {
            let this_val = box_or_pass(b, actx.vars, state, this_reg);
            let regs = live_boxed(actx, state);
            flush_boxed(b, actx, state, &regs);
            let res = call_helper(
                b,
                actx.cc,
                actx.helpers.array_pop,
                &[actx.exec_ctx, this_val],
            );
            reload_boxed(b, actx, state, &regs);
            if meta_is_float(&proto.register_meta, dest) {
                let f = unbox_f64_coerce(b, res);
                b.def_var(actx.vars[dest], f);
            } else {
                b.def_var(actx.vars[dest], res);
            }
            return;
        }
    }

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

    reload_boxed(b, actx, state, &regs);

    if meta_is_float(&proto.register_meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(actx.vars[dest], f);
    } else {
        b.def_var(actx.vars[dest], res);
    }
}
