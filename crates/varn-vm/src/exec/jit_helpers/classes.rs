//! Class construction and the class member protocol: field declaration,
//! inheritance, and static/instance member installation.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

/// Argument block for [`jit_class_member_op`], passed by pointer because the
/// CLIF backend cannot spread four mixed-width values across registers here.
#[repr(C)]
pub struct JitClassMemberArgs {
    pub class_val: VmValue,
    pub fn_val: VmValue,
    pub name_idx: usize,
    pub kind: u8,
}

pub(crate) extern "C" fn jit_get_super(ctx: *mut ExecCtx, name_idx: usize) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let closure_ref = ctx_ref.frames[frame_idx].closure();
        let base = ctx_ref.frames[frame_idx].base;
        let this_val = ctx_ref.stack[base];
        let name_nv = closure_ref.constants[name_idx];
        let name = ctx_ref.heap.str_val(name_nv).expect("non-string const");

        let cls = ctx_ref.frames[frame_idx]
            .current_class
            .clone()
            .or_else(|| crate::exec::props::get_class(this_val, &ctx_ref.heap))
            .expect("GetSuper: 'this' has no class");
        let class_nv = ctx_ref.heap.intern(varn_types::Value::Class(cls));
        match crate::exec::class::op_get_super(class_nv, &name, this_val, &mut ctx_ref.heap) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_declare_field(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    class_val: VmValue,
    name_idx: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let key_nv = closure_ref.constants[name_idx];
        let key = ctx_ref.heap.str_val(key_nv).expect("non-string const");
        if let Err(e) = crate::exec::class::op_declare_field(class_val, &key, &mut ctx_ref.heap) {
            jit_propagate_error(ctx_ref, e);
        }
    }
}

pub(crate) extern "C" fn jit_make_class(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    super_val: VmValue,
    name_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let name_nv = closure_ref.constants[name_idx];
        let name = ctx_ref.heap.str_val(name_nv).expect("non-string const");
        let cls = crate::exec::class::op_class(&name, &mut ctx_ref.heap);
        if !super_val.is_null() {
            if let Err(e) = crate::exec::class::op_inherit(cls, super_val, &mut ctx_ref.heap) {
                jit_propagate_error(ctx_ref, e);
            }
        }
        cls
    }
}

pub(crate) extern "C" fn jit_inherit(ctx: *mut ExecCtx, class_val: VmValue, super_val: VmValue) {
    unsafe {
        let ctx_ref = &mut *ctx;
        if let Err(e) = crate::exec::class::op_inherit(class_val, super_val, &mut ctx_ref.heap) {
            jit_propagate_error(ctx_ref, e);
        }
    }
}

pub(crate) extern "C" fn jit_class_member_op(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    args: *const std::ffi::c_void,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let args = &*(args as *const JitClassMemberArgs);
        let closure_ref = &*closure;
        let key_nv = closure_ref.constants[args.name_idx];
        let key = ctx_ref.heap.str_val(key_nv).expect("non-string const");

        let res = match args.kind {
            0 => {
                crate::exec::class::op_method(args.class_val, &key, args.fn_val, &mut ctx_ref.heap)
            }
            1 => crate::exec::class::op_define_static(
                args.class_val,
                &key,
                args.fn_val,
                &mut ctx_ref.heap,
            ),
            2 => crate::exec::class::op_define_getter(
                args.class_val,
                &key,
                args.fn_val,
                &mut ctx_ref.heap,
            ),
            3 => crate::exec::class::op_define_setter(
                args.class_val,
                &key,
                args.fn_val,
                &mut ctx_ref.heap,
            ),
            4 => crate::exec::class::op_define_static_getter(
                args.class_val,
                &key,
                args.fn_val,
                &mut ctx_ref.heap,
            ),
            5 => crate::exec::class::op_define_static_setter(
                args.class_val,
                &key,
                args.fn_val,
                &mut ctx_ref.heap,
            ),
            _ => panic!("Unknown class member op kind: {}", args.kind),
        };
        if let Err(e) = res {
            jit_propagate_error(ctx_ref, e);
        }
    }
}

