use cranelift_codegen::ir::{
    types, AbiParam, ExtFuncData, ExternalName, Function, InstBuilder, MemFlagsData, Signature,
    UserExternalName, UserFuncName, Value,
};
use cranelift_codegen::isa::{CallConv, OwnedTargetIsa};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use varn_types::register_meta::SlotKind;
use varn_types::FunctionProto;

use super::emit::retag_raw_return;
use super::native_abi::{NativeClass, NativeShape};
use super::piece::{compile_piece, CompiledPiece};
use crate::JitHelpers;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    Native,
    Framed,
}

pub(super) fn raw_signature(proto: &FunctionProto, nparams: usize, cc: CallConv) -> Signature {
    let mut sig = Signature::new(cc);
    for _ in 0..4 {
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

pub(crate) fn wrapper_returns_via_sret(cc: CallConv) -> bool {
    cc == CallConv::WindowsFastcall
}

pub(crate) fn wrapper_signature(cc: CallConv) -> Signature {
    let mut sig = Signature::new(cc);
    if wrapper_returns_via_sret(cc) {
        sig.params.push(AbiParam::special(
            types::I64,
            cranelift_codegen::ir::ArgumentPurpose::StructReturn,
        ));
    }
    for _ in 0..4 {
        sig.params.push(AbiParam::new(types::I64));
    }
    if !wrapper_returns_via_sret(cc) {
        sig.returns.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
    }
    sig
}

pub(super) fn build_wrapper(
    proto: &FunctionProto,
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    activation: Activation,
    osr: bool,
) -> Result<CompiledPiece, String> {
    let cc = isa.default_call_conv();
    let is_windows = wrapper_returns_via_sret(cc);
    let mut func = Function::with_name_signature(UserFuncName::user(0, 1), wrapper_signature(cc));
    let nparams = if osr {
        0
    } else {
        proto.arity.saturating_sub(1)
    };
    let shape = NativeShape::of_proto(proto);
    let raw_sig = func.import_signature(match activation {
        Activation::Native => shape.signature(),
        Activation::Framed => raw_signature(proto, nparams, cc),
    });
    let raw_name = func.declare_imported_user_function(UserExternalName::new(0, 0));
    let raw_ref = func.import_function(ExtFuncData {
        name: ExternalName::user(raw_name),
        signature: raw_sig,
        colocated: true,
        patchable: false,
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

    let layout = varn_types::register_meta::FrameLayout::for_proto(proto);
    let homes = super::homes::Homes {
        exec_ctx,
        base,
        layout: &layout,
        offsets: &helpers.frame_layout,
    };
    let result = match activation {
        Activation::Native => {
            let mut args = vec![exec_ctx, closure];
            for (r, class) in shape.params.iter().enumerate() {
                let boxed = homes.load(&mut b, r);
                push_native_arg(&mut b, &mut args, *class, kind_of(proto, r), boxed);
            }
            let call = b.ins().call(raw_ref, &args);
            let res = b.inst_results(call).to_vec();
            native_result_boxed(&mut b, shape.ret, proto.return_kind, &res)
        }
        Activation::Framed => {
            let mut args = vec![stack_ptr, closure, base, exec_ctx];
            for i in 0..nparams {
                let boxed = homes.load(&mut b, 1 + i);
                let un = match proto.param_kinds.get(i) {
                    Some(SlotKind::Int) => super::emit::unbox_int(&mut b, boxed),
                    Some(SlotKind::Bool) => super::emit::unbox_bool(&mut b, boxed),
                    _ if proto.param_kinds.get(i) == Some(&SlotKind::Float)
                        || super::emit::meta_is_float(&proto.register_meta, 1 + i) =>
                    {
                        super::emit::unbox_f64_coerce(&mut b, boxed)
                    }
                    Some(SlotKind::Float)
                    | Some(SlotKind::Str)
                    | Some(SlotKind::Ref)
                    | Some(SlotKind::Dynamic)
                    | None => b.ins().isplit(boxed).1,
                };
                args.push(un);
            }
            let call = b.ins().call(raw_ref, &args);
            match proto.return_kind {
                SlotKind::Int | SlotKind::Bool | SlotKind::Float => {
                    let raw_res = b.inst_results(call)[0];
                    retag_raw_return(&mut b, raw_res, proto.return_kind)
                }
                SlotKind::Str | SlotKind::Ref | SlotKind::Dynamic => b.ins().load(
                    types::I128,
                    MemFlagsData::trusted(),
                    exec_ctx,
                    helpers.jit_native_result_offset as i32,
                ),
            }
        }
    };
    let (tag, payload) = b.ins().isplit(result);
    if let Some(sret) = sret_ptr {
        b.ins().store(MemFlagsData::trusted(), tag, sret, 0);
        b.ins().store(MemFlagsData::trusted(), payload, sret, 8);
        b.ins().return_(&[]);
    } else {
        b.ins().return_(&[tag, payload]);
    }
    b.finalize(isa.frontend_config());
    compile_piece(func, isa)
}

fn kind_of(proto: &FunctionProto, reg: usize) -> SlotKind {
    match reg {
        0 => SlotKind::Dynamic,
        r => proto.param_kinds[r - 1],
    }
}

pub(crate) fn push_native_arg(
    b: &mut FunctionBuilder,
    args: &mut Vec<Value>,
    class: NativeClass,
    kind: SlotKind,
    boxed: Value,
) {
    match class {
        NativeClass::Word if kind == SlotKind::Bool => args.push(super::emit::unbox_bool(b, boxed)),
        NativeClass::Word => args.push(super::emit::unbox_int(b, boxed)),
        NativeClass::Float => args.push(super::emit::unbox_f64_coerce(b, boxed)),
        NativeClass::Boxed => {
            let (tag, payload) = b.ins().isplit(boxed);
            args.push(tag);
            args.push(payload);
        }
    }
}

pub(crate) fn native_result_boxed(
    b: &mut FunctionBuilder,
    class: NativeClass,
    kind: SlotKind,
    res: &[Value],
) -> Value {
    match class {
        NativeClass::Word | NativeClass::Float => retag_raw_return(b, res[0], kind),
        NativeClass::Boxed => b.ins().iconcat(res[0], res[1]),
    }
}
