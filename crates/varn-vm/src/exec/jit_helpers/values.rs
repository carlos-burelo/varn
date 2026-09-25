//! Value-level helpers: constants, upvalues, closures, and the arithmetic and
//! comparison operators compiled code cannot inline.
//!
//! Globals are absent on purpose: `clif::globals` emits the indexed load and
//! store inline off `ExecCtx.globals`, and the compiler emits those indexed
//! forms directly — a name-keyed `LoadGlobal` only survives for a genuinely
//! dynamic name, which bails.
//!
//! Everything here is a pure value operation over the running `ExecCtx` —
//! no frame is pushed and no call is made.

use crate::exec::closures::UpvalueSrc;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

pub(crate) extern "C" fn jit_load_const(
    closure: *const crate::closure::VmClosure,
    idx: usize,
) -> VmValue {
    unsafe {
        let closure_ref = &*closure;
        closure_ref.constants[idx]
    }
}

pub(crate) extern "C" fn jit_eq(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) -> u64 {
    unsafe {
        let ctx_ref = &mut *ctx;
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        if crate::exec::compare::eq(a, b, &ctx_ref.heap) {
            1
        } else {
            0
        }
    }
}

pub(crate) extern "C" fn jit_neq(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) -> u64 {
    unsafe {
        let ctx_ref = &mut *ctx;
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        if crate::exec::compare::neq(a, b, &ctx_ref.heap) {
            1
        } else {
            0
        }
    }
}

pub(crate) extern "C" fn jit_lt(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) -> u64 {
    unsafe {
        let ctx_ref = &mut *ctx;
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        if crate::exec::compare::lt_heap(a, b, &ctx_ref.heap) {
            1
        } else {
            0
        }
    }
}

pub(crate) extern "C" fn jit_lte(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) -> u64 {
    unsafe {
        let ctx_ref = &mut *ctx;
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        if crate::exec::compare::lte_heap(a, b, &ctx_ref.heap) {
            1
        } else {
            0
        }
    }
}

pub(crate) extern "C" fn jit_gt(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) -> u64 {
    unsafe {
        let ctx_ref = &mut *ctx;
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        if crate::exec::compare::gt_heap(a, b, &ctx_ref.heap) {
            1
        } else {
            0
        }
    }
}

pub(crate) extern "C" fn jit_gte(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) -> u64 {
    unsafe {
        let ctx_ref = &mut *ctx;
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        if crate::exec::compare::gte_heap(a, b, &ctx_ref.heap) {
            1
        } else {
            0
        }
    }
}

pub(crate) extern "C" fn jit_add(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        match crate::exec::arith::add(a, b, &mut ctx_ref.heap) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => super::construct::jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_sub(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        match crate::exec::arith::sub(a, b, &mut ctx_ref.heap) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => super::construct::jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_mul(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        match crate::exec::arith::mul(a, b, &mut ctx_ref.heap) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => super::construct::jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_to_string(ctx: *mut ExecCtx, v_tag: u64, v_payload: u64) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let v = VmValue::from_raw_parts(v_tag, v_payload);
        ctx_ref.jit_native_result = crate::exec::strings::to_string(v, &mut ctx_ref.heap);
    }
}

pub(crate) extern "C" fn jit_load_upvalue(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    uv_idx: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        ctx_ref.jit_native_result = closure_ref.upvalues[uv_idx].read(&ctx_ref.stack);
    }
}

pub(crate) extern "C" fn jit_store_upvalue(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    uv_idx: usize,
    val_tag: u64,
    val_payload: u64,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let val = VmValue::from_raw_parts(val_tag, val_payload);
        if let Err(e) = closure_ref.upvalues[uv_idx].write(val, &mut ctx_ref.stack) {
            super::construct::jit_propagate_error(ctx_ref, e);
        }
    }
}
/// `MakeClosure` out of the lowering from bytecode: the descriptor follows
/// the opcode at `ip_offset` of the running closure's code. `base` is the
/// compiled caller's activation, whose registers a local upvalue captures.
pub(crate) extern "C" fn jit_make_closure(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    ip_offset: usize,
    base: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let code = &closure_ref.proto.chunk.code;
        let uv_count = (code[ip_offset + 1] & 0xFF) as usize;
        let proto_idx = code[ip_offset + 2] as usize;
        let descs = &code[ip_offset + 3..ip_offset + 3 + uv_count];
        let upvalues = descs.iter().map(|&w| UpvalueSrc::from_bytecode(w));
        match ctx_ref.make_closure(closure_ref, proto_idx, base, upvalues) {
            Ok(val) => ctx_ref.jit_native_result = val,
            Err(e) => super::construct::jit_propagate_error(ctx_ref, e),
        }
    }
}

/// `MakeClosure` out of the lowering from typed SSA: function constant
/// `proto_idx`, and `count` upvalue source words at `descs`
/// ([`UpvalueSrc::from_word`]). `base` is the compiled caller's activation.
pub(crate) extern "C" fn jit_make_closure_window(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    base: usize,
    proto_idx: usize,
    descs: *const u64,
    count: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let words: &[u64] = if count == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(descs, count)
        };
        let upvalues = words.iter().map(|&w| UpvalueSrc::from_word(w));
        match ctx_ref.make_closure(closure_ref, proto_idx, base, upvalues) {
            Ok(val) => ctx_ref.jit_native_result = val,
            Err(e) => super::construct::jit_propagate_error(ctx_ref, e),
        }
    }
}
