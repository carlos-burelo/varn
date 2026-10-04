//! The native activation ABI: how one compiled body calls another without a
//! VM frame between them.
//!
//! `raw(ctx, closure, r0, r1, …) -> ret` in Cranelift's `tail` convention.
//! Each register travels in its class's machine form — an `int`/`bool` as
//! one `i64`, a `float` as one `f64`, anything boxed as its `(tag, payload)`
//! words — and so does the return. `r0` is the receiver slot (`this`, or the
//! callee placeholder of a plain call) and is always boxed.

use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_codegen::isa::CallConv;
use varn_types::register_meta::SlotKind;
use varn_types::FunctionProto;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeClass {
    Word,
    Float,
    Boxed,
}

impl NativeClass {
    pub(crate) fn of(kind: SlotKind) -> Self {
        match kind {
            SlotKind::Int | SlotKind::Bool => NativeClass::Word,
            SlotKind::Float => NativeClass::Float,
            SlotKind::Str | SlotKind::Ref | SlotKind::Dynamic => NativeClass::Boxed,
        }
    }

    fn push(self, params: &mut Vec<AbiParam>) {
        match self {
            NativeClass::Word => params.push(AbiParam::new(types::I64)),
            NativeClass::Float => params.push(AbiParam::new(types::F64)),
            NativeClass::Boxed => {
                params.push(AbiParam::new(types::I64));
                params.push(AbiParam::new(types::I64));
            }
        }
    }

    pub(crate) fn words(self) -> usize {
        match self {
            NativeClass::Word | NativeClass::Float => 1,
            NativeClass::Boxed => 2,
        }
    }
}

pub(crate) const CALL_CONV: CallConv = CallConv::Tail;

/// The machine shape of one body: its registers `r0..arity` and its return.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeShape {
    pub params: Vec<NativeClass>,
    pub ret: NativeClass,
}

impl NativeShape {
    pub(crate) fn new(param_kinds: &[SlotKind], return_kind: SlotKind) -> Self {
        let mut params = Vec::with_capacity(param_kinds.len() + 1);
        params.push(NativeClass::Boxed);
        params.extend(param_kinds.iter().map(|k| NativeClass::of(*k)));
        NativeShape {
            params,
            ret: NativeClass::of(return_kind),
        }
    }

    pub(crate) fn of_proto(proto: &FunctionProto) -> Self {
        Self::new(&proto.param_kinds, proto.return_kind)
    }

    pub(crate) fn signature(&self) -> Signature {
        let mut sig = Signature::new(CALL_CONV);
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        for c in &self.params {
            c.push(&mut sig.params);
        }
        self.ret.push(&mut sig.returns);
        sig
    }
}
