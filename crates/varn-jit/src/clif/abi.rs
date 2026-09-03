//! The two calling conventions a compilation produces, and the wrapper that
//! bridges them.
//!
//! * RAW — unboxed, the shape the body is actually lowered against.
//! * `JitFn` — the four fixed words every VM-side caller knows how to invoke.
//!
//! Keeping both here means the ABI can be read in one place: `lower::body`
//! consumes [`raw_signature`] and never restates the layout, and the wrapper
//! is the only code that knows how a boxed argument becomes a raw one.

use cranelift_codegen::ir::{
    types, AbiParam, Function, InstBuilder, MemFlags, Signature, UserFuncName,
};
use cranelift_codegen::isa::OwnedTargetIsa;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use varn_types::register_meta::SlotKind;
use varn_types::FunctionProto;

use super::emit::retag_raw_return;
use super::piece::{compile_piece, CompiledPiece};
use crate::JitHelpers;

/// Raw signature: `fn(exec_ctx, [base, closure], arg × nparams) -> i64`.
/// Int-declared args arrive unboxed; everything else arrives as its boxed
/// VmValue bits. `exec_ctx` is only dereferenced by the heap-walking ops and
/// the slow helpers. Frame-aware functions (they allocate and/or take a
/// `this` receiver) carry two extra parameters: `base` (this frame's
/// register-0 index into `ctx.stack`, for flushing heap-typed registers to
/// their home slots at a safepoint and for reading the receiver from
/// `stack[base+0]`) and `closure` (this function's `VmClosure*`, needed by
/// shape-driven object construction).
pub(super) fn raw_signature(
    proto: &FunctionProto,
    nparams: usize,
    isa: &OwnedTargetIsa,
    frame_aware: bool,
) -> Signature {
    let mut sig = Signature::new(isa.default_call_conv());
    let extra = if frame_aware { 3 } else { 0 };
    for _ in 0..(1 + extra) {
        sig.params.push(AbiParam::new(types::I64));
    }
    for i in 0..nparams {
        let is_float = proto.param_kinds.get(i) == Some(&SlotKind::Float)
            || super::emit::meta_is_float(&proto.register_meta, 1 + i);
        if is_float {
            sig.params.push(AbiParam::new(types::F64));
        } else {
            sig.params.push(AbiParam::new(types::I64));
        }
    }
    if proto.return_kind == SlotKind::Int || proto.return_kind == SlotKind::Bool {
        sig.returns.push(AbiParam::new(types::I64));
    } else if proto.return_kind == SlotKind::Float {
        sig.returns.push(AbiParam::new(types::F64));
    }
    sig
}

/// Wrapper with the template `JitFn` ABI:
/// `(stack_ptr, closure, base, exec_ctx) -> boxed VmValue`.
///
/// Kept for the OSR entry too, rather than exposing `raw` directly: the
/// wrapper is what consumes the caller-prepush flag (every JIT prologue must)
/// and what re-tags an unboxed i48 return. An OSR body returns through the
/// same `Return` opcodes as any other, so it needs both.
pub(super) fn build_wrapper(
    proto: &FunctionProto,
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    frame_aware: bool,
    osr: bool,
) -> Result<CompiledPiece, String> {
    // Mirrors `lower_raw`: the OSR raw takes no arguments, so the wrapper
    // imports that signature and loads none from the stack.
    let nparams = if osr {
        0
    } else {
        proto.arity.saturating_sub(1)
    };
    let is_windows = isa.default_call_conv() == cranelift_codegen::isa::CallConv::WindowsFastcall;
    let mut sig = Signature::new(isa.default_call_conv());
    if is_windows {
        sig.params.push(AbiParam::special(
            types::I64,
            cranelift_codegen::ir::ArgumentPurpose::StructReturn,
        ));
    }
    for _ in 0..4 {
        sig.params.push(AbiParam::new(types::I64));
    }
    if !is_windows {
        sig.returns.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
    }

    let mut func = Function::with_name_signature(UserFuncName::user(0, 1), sig);
    let raw_sig = func.import_signature(raw_signature(proto, nparams, isa, frame_aware));
    let raw_name =
        func.declare_imported_user_function(cranelift_codegen::ir::UserExternalName::new(0, 0));
    let raw_ref = func.import_function(cranelift_codegen::ir::ExtFuncData {
        name: cranelift_codegen::ir::ExternalName::user(raw_name),
        signature: raw_sig,
        colocated: true,
    });

    let mut fb_ctx = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut fb_ctx);
    let block = b.create_block();
    b.append_block_params_for_function_params(block);
    b.switch_to_block(block);
    b.seal_block(block);

    let (sret_ptr, stack_ptr, closure, base, exec_ctx) = {
        let p = b.block_params(block);
        if is_windows {
            (Some(p[0]), p[1], p[2], p[3], p[4])
        } else {
            (None, p[0], p[1], p[2], p[3])
        }
    };

    // Protocol: every JIT prologue consumes the caller-prepush flag.
    let zero32 = b.ins().iconst(types::I64, 0);
    b.ins().store(
        MemFlags::trusted(),
        zero32,
        exec_ctx,
        helpers.frame_prepushed_offset as i32,
    );

    // Boxed args live at stack[base + 1 + i].
    let base_bytes = b.ins().imul_imm(base, 16);
    let arg_base = b.ins().iadd(stack_ptr, base_bytes);
    let mut args = Vec::with_capacity(4 + nparams);
    if frame_aware {
        args.push(stack_ptr);
        args.push(closure);
        args.push(base);
    }
    args.push(exec_ctx);
    for i in 0..nparams {
        let boxed = b.ins().load(
            types::I128,
            MemFlags::trusted(),
            arg_base,
            ((1 + i) * 16) as i32,
        );
        if proto.param_kinds.get(i) == Some(&SlotKind::Int) {
            let un = super::emit::wrap_i48(&mut b, boxed);
            args.push(un);
        } else if proto.param_kinds.get(i) == Some(&SlotKind::Float)
            || super::emit::meta_is_float(&proto.register_meta, 1 + i)
        {
            let un = super::emit::unbox_f64_coerce(&mut b, boxed);
            args.push(un);
        } else if proto.param_kinds.get(i) == Some(&SlotKind::Bool) {
            let un = super::emit::unbox_bool(&mut b, boxed);
            args.push(un);
        } else {
            let (_tag, payload) = b.ins().isplit(boxed);
            args.push(payload);
        }
    }

    let result = if proto.return_kind == SlotKind::Int
        || proto.return_kind == SlotKind::Bool
        || proto.return_kind == SlotKind::Float
    {
        let call = b.ins().call(raw_ref, &args);
        let raw_res = b.inst_results(call)[0];
        retag_raw_return(&mut b, raw_res, proto.return_kind)
    } else {
        b.ins().call(raw_ref, &args);
        b.ins().load(
            types::I128,
            MemFlags::trusted(),
            exec_ctx,
            helpers.jit_native_result_offset as i32,
        )
    };
    let (tag, payload) = b.ins().isplit(result);
    if let Some(sret) = sret_ptr {
        b.ins().store(MemFlags::trusted(), tag, sret, 0);
        b.ins().store(MemFlags::trusted(), payload, sret, 8);
        b.ins().return_(&[]);
    } else {
        b.ins().return_(&[tag, payload]);
    }
    b.finalize();
    compile_piece(func, isa)
}
