//! Property get and set, in the four shapes codegen emits.
//!
//! The `_flat` variants take their arguments unpacked instead of through a
//! struct, which is what lets the CLIF backend call them without building an
//! argument block first.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

pub(crate) extern "C" fn jit_get_property(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    args: *const varn_jit::JitGetPropertyArgs,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let args = &*args;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;
        let base = ctx_ref.frames[frame_idx].base;

        ctx_ref.frames[frame_idx].ip = args.ip;

        let res = ctx_ref.exec_get_property_reg(
            args.obj,
            args.name_idx,
            args.cs_idx,
            args.dest,
            base,
            frame_idx,
            closure_ref,
        );

        match res {
            Ok(true) => {
                if let Err(e) = ctx_ref.run_until_inner(caller_depth) {
                    jit_propagate_error(ctx_ref, e);
                }
            }
            Ok(false) => {}
            Err(e) => jit_propagate_error(ctx_ref, e),
        }

        ctx_ref.stack[base + args.dest]
    }
}

/// Flat-argument shim over [`jit_get_property`] so the CLIF backend can call
/// it with plain scalars instead of building a `JitGetPropertyArgs` struct in
/// a stack slot. Same semantics (may run a getter, hence may GC).
#[allow(clippy::too_many_arguments)]
pub(crate) extern "C" fn jit_get_property_flat(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    base: usize,
    obj: VmValue,
    name_idx: usize,
    cs_idx: usize,
    dest: usize,
    ip: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;

        ctx_ref.frames[frame_idx].ip = ip;

        let res = ctx_ref.exec_get_property_reg(
            obj,
            name_idx,
            cs_idx,
            dest,
            base,
            frame_idx,
            closure_ref,
        );

        match res {
            Ok(true) => {
                if let Err(e) = ctx_ref.run_until_inner(caller_depth) {
                    jit_propagate_error(ctx_ref, e);
                }
            }
            Ok(false) => {}
            Err(e) => jit_propagate_error(ctx_ref, e),
        }

        ctx_ref.stack[base + dest]
    }
}

/// Flat-argument shim over [`jit_set_property`] for the CLIF backend (may run
/// a setter, hence may GC).
#[allow(clippy::too_many_arguments)]
pub(crate) extern "C" fn jit_set_property_flat(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    obj: VmValue,
    val: VmValue,
    name_idx: usize,
    cs_idx: usize,
    ip: usize,
) {
    let args = varn_jit::JitSetPropertyArgs {
        obj,
        val,
        name_idx,
        cs_idx,
        ip,
    };
    jit_set_property(ctx, closure, &args)
}

pub(crate) extern "C" fn jit_set_property(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    args: *const varn_jit::JitSetPropertyArgs,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let args = &*args;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;
        let base = ctx_ref.frames[frame_idx].base;

        ctx_ref.frames[frame_idx].ip = args.ip;

        let res = ctx_ref.exec_set_property_reg(
            args.obj,
            args.val,
            args.name_idx,
            args.cs_idx,
            base,
            frame_idx,
            closure_ref,
        );

        match res {
            Ok(true) => {
                if let Err(e) = ctx_ref.run_until_inner(caller_depth) {
                    jit_propagate_error(ctx_ref, e);
                }
            }
            Ok(false) => {}
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_get_fixed_field(
    ctx: *mut ExecCtx,
    obj: VmValue,
    slot: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match crate::exec::props::get_fixed_field(obj, slot, &mut ctx_ref.heap) {
            Ok(val) => val,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_set_fixed_field(
    ctx: *mut ExecCtx,
    obj: VmValue,
    slot: usize,
    val: VmValue,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        if let Err(e) = crate::exec::props::set_fixed_field(obj, slot, val, &mut ctx_ref.heap) {
            jit_propagate_error(ctx_ref, e);
        }
    }
}

pub(crate) extern "C" fn jit_get_property_maybe_stub(
    ctx: *mut ExecCtx,
    obj: VmValue,
    name_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let closure_ref = ctx_ref.frames[frame_idx].closure();
        let name_nv = closure_ref.constants[name_idx];
        let name = ctx_ref.heap.str_val(name_nv).expect("non-string const");
        crate::exec::props::get_property_maybe(obj, &name, &mut ctx_ref.heap)
    }
}

pub(crate) extern "C" fn jit_get_symbol(
    ctx: *mut ExecCtx,
    obj: VmValue,
    sym_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let closure_ref = ctx_ref.frames[frame_idx].closure();
        let sym_nv = closure_ref.constants[sym_idx];
        let sym_val = ctx_ref.heap.extract(sym_nv);
        match sym_val {
            varn_types::Value::Symbol(s) => {
                match crate::exec::advanced::get_symbol_property(obj, s, &mut ctx_ref.heap) {
                    Ok(v) => v,
                    Err(e) => jit_propagate_error(ctx_ref, e),
                }
            }
            _ => panic!("GetSymbol: non-symbol constant"),
        }
    }
}

pub(crate) extern "C" fn jit_bind_method(
    ctx: *mut ExecCtx,
    obj: VmValue,
    name_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let closure_ref = ctx_ref.frames[frame_idx].closure();
        let key_nv = closure_ref.constants[name_idx];
        let key = ctx_ref.heap.str_val(key_nv).expect("non-string const");
        let method = match crate::exec::props::get_property(obj, &key, &mut ctx_ref.heap) {
            Ok(m) => m,
            Err(e) => jit_propagate_error(ctx_ref, e),
        };
        match crate::exec::advanced::bind_method(obj, method, &mut ctx_ref.heap) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}
