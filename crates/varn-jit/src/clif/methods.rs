//! Method call lowering for CLIF: `CallMethod` and `InvokeVirtual`.
//! Handled via fast-path flat dispatch helpers that manage VM frames,
//! GC safepoints, and register reloads.

use cranelift_codegen::ir::{types, InstBuilder};
use cranelift_frontend::FunctionBuilder;
use varn_types::FunctionProto;

use super::alloc::{
    box_or_load_home, def_result, flush_boxed, frame_base_addr, live_boxed, reload_boxed,
    store_home, AllocCtx,
};
use super::emit::{box_bool, call_helper, call_helper_void};
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

    let fb = frame_base_addr(b, actx);
    for i in 0..argc {
        store_home(b, actx, state, fb, arg_start + i);
    }

    // Fast path: known intrinsic Array push / pop methods
    if let Some(name) = proto.chunk.constants.get(name_idx).and_then(|p| p.as_str()) {
        if name == varn_core::MemberKey::Push.as_str() && argc == 1 {
            let this_val = box_or_load_home(b, actx, state, this_reg);
            let val = box_or_load_home(b, actx, state, arg_start);
            let (this_tag, this_payload) = b.ins().isplit(this_val);
            let (val_tag, val_payload) = b.ins().isplit(val);
            let regs = live_boxed(actx, state);
            flush_boxed(b, actx, state, &regs);
            call_helper_void(
                b,
                actx.cc,
                actx.helpers.array_push,
                &[actx.exec_ctx, this_tag, this_payload, val_tag, val_payload],
            );
            reload_boxed(b, actx, state, &regs);
            let null_v = super::emit::box_null(b);
            def_result(b, actx, dest, null_v);
            return;
        } else if name == varn_core::MemberKey::Pop.as_str() && argc == 0 {
            let this_val = box_or_load_home(b, actx, state, this_reg);
            let (this_tag, this_payload) = b.ins().isplit(this_val);
            let regs = live_boxed(actx, state);
            flush_boxed(b, actx, state, &regs);
            call_helper_void(
                b,
                actx.cc,
                actx.helpers.array_pop,
                &[actx.exec_ctx, this_tag, this_payload],
            );
            reload_boxed(b, actx, state, &regs);
            let res = b.ins().load(
                types::I128,
                cranelift_codegen::ir::MemFlags::trusted(),
                actx.exec_ctx,
                actx.helpers.jit_native_result_offset as i32,
            );
            def_result(b, actx, dest, res);
            return;
        } else if name == varn_core::MemberKey::StartsWith.as_str() && argc == 1 {
            let this_val = box_or_load_home(b, actx, state, this_reg);
            let val = box_or_load_home(b, actx, state, arg_start);
            let (this_tag, this_payload) = b.ins().isplit(this_val);
            let (val_tag, val_payload) = b.ins().isplit(val);
            let res = call_helper(
                b,
                actx.cc,
                actx.helpers.str_starts_with_intrinsic,
                &[actx.exec_ctx, this_tag, this_payload, val_tag, val_payload],
            );
            let boxed = box_bool(b, res);
            def_result(b, actx, dest, boxed);
            return;
        } else if name == varn_core::MemberKey::EndsWith.as_str() && argc == 1 {
            let this_val = box_or_load_home(b, actx, state, this_reg);
            let val = box_or_load_home(b, actx, state, arg_start);
            let (this_tag, this_payload) = b.ins().isplit(this_val);
            let (val_tag, val_payload) = b.ins().isplit(val);
            let res = call_helper(
                b,
                actx.cc,
                actx.helpers.str_ends_with_intrinsic,
                &[actx.exec_ctx, this_tag, this_payload, val_tag, val_payload],
            );
            let boxed = box_bool(b, res);
            def_result(b, actx, dest, boxed);
            return;
        }
    }

    let this_val = box_or_load_home(b, actx, state, this_reg);
    let (this_tag, this_payload) = b.ins().isplit(this_val);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let ni = b.ins().iconst(types::I64, name_idx as i64);
    let ci = b.ins().iconst(types::I64, cs as i64);
    let ast = b.ins().iconst(types::I64, arg_start as i64);
    let ac = b.ins().iconst(types::I64, argc as i64);
    let de = b.ins().iconst(types::I64, dest as i64);
    let ipv = b.ins().iconst(types::I64, next_ip as i64);

    call_helper_void(
        b,
        actx.cc,
        actx.helpers.call_method_flat,
        &[
            actx.exec_ctx,
            actx.closure,
            actx.base,
            this_tag,
            this_payload,
            ni,
            ci,
            ast,
            ac,
            de,
            ipv,
        ],
    );

    reload_boxed(b, actx, state, &regs);

    let res = b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
    );

    def_result(b, actx, dest, res);
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
            let this_val = box_or_load_home(b, actx, state, this_reg);
            let val = box_or_load_home(b, actx, state, arg_start);
            let (this_tag, this_payload) = b.ins().isplit(this_val);
            let (val_tag, val_payload) = b.ins().isplit(val);
            let regs = live_boxed(actx, state);
            flush_boxed(b, actx, state, &regs);
            call_helper_void(
                b,
                actx.cc,
                actx.helpers.array_push,
                &[actx.exec_ctx, this_tag, this_payload, val_tag, val_payload],
            );
            reload_boxed(b, actx, state, &regs);
            let null_v = super::emit::box_null(b);
            def_result(b, actx, dest, null_v);
            return;
        } else if name == varn_core::MemberKey::Pop.as_str() && argc == 0 {
            let this_val = box_or_load_home(b, actx, state, this_reg);
            let (this_tag, this_payload) = b.ins().isplit(this_val);
            let regs = live_boxed(actx, state);
            flush_boxed(b, actx, state, &regs);
            call_helper_void(
                b,
                actx.cc,
                actx.helpers.array_pop,
                &[actx.exec_ctx, this_tag, this_payload],
            );
            reload_boxed(b, actx, state, &regs);
            let res = b.ins().load(
                types::I128,
                cranelift_codegen::ir::MemFlags::trusted(),
                actx.exec_ctx,
                actx.helpers.jit_native_result_offset as i32,
            );
            def_result(b, actx, dest, res);
            return;
        } else if name == varn_core::MemberKey::StartsWith.as_str() && argc == 1 {
            let this_val = box_or_load_home(b, actx, state, this_reg);
            let val = box_or_load_home(b, actx, state, arg_start);
            let (this_tag, this_payload) = b.ins().isplit(this_val);
            let (val_tag, val_payload) = b.ins().isplit(val);
            let regs = live_boxed(actx, state);
            flush_boxed(b, actx, state, &regs);
            call_helper_void(
                b,
                actx.cc,
                actx.helpers.str_starts_with,
                &[actx.exec_ctx, this_tag, this_payload, val_tag, val_payload],
            );
            reload_boxed(b, actx, state, &regs);
            let res = b.ins().load(
                types::I128,
                cranelift_codegen::ir::MemFlags::trusted(),
                actx.exec_ctx,
                actx.helpers.jit_native_result_offset as i32,
            );
            def_result(b, actx, dest, res);
            return;
        } else if name == varn_core::MemberKey::EndsWith.as_str() && argc == 1 {
            let this_val = box_or_load_home(b, actx, state, this_reg);
            let val = box_or_load_home(b, actx, state, arg_start);
            let (this_tag, this_payload) = b.ins().isplit(this_val);
            let (val_tag, val_payload) = b.ins().isplit(val);
            let regs = live_boxed(actx, state);
            flush_boxed(b, actx, state, &regs);
            call_helper_void(
                b,
                actx.cc,
                actx.helpers.str_ends_with,
                &[actx.exec_ctx, this_tag, this_payload, val_tag, val_payload],
            );
            reload_boxed(b, actx, state, &regs);
            let res = b.ins().load(
                types::I128,
                cranelift_codegen::ir::MemFlags::trusted(),
                actx.exec_ctx,
                actx.helpers.jit_native_result_offset as i32,
            );
            def_result(b, actx, dest, res);
            return;
        }
    }

    let fb = frame_base_addr(b, actx);
    for i in 0..argc {
        store_home(b, actx, state, fb, arg_start + i);
    }
    let this_val = box_or_load_home(b, actx, state, this_reg);
    let (this_tag, this_payload) = b.ins().isplit(this_val);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let ni = b.ins().iconst(types::I64, name_idx as i64);
    let ast = b.ins().iconst(types::I64, arg_start as i64);
    let ac = b.ins().iconst(types::I64, argc as i64);
    let de = b.ins().iconst(types::I64, dest as i64);
    let ipv = b.ins().iconst(types::I64, next_ip as i64);

    call_helper_void(
        b,
        actx.cc,
        actx.helpers.invoke_virtual_flat,
        &[
            actx.exec_ctx,
            actx.closure,
            this_tag,
            this_payload,
            ni,
            ast,
            ac,
            de,
            ipv,
        ],
    );

    reload_boxed(b, actx, state, &regs);

    let res = b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
    );

    def_result(b, actx, dest, res);
}
