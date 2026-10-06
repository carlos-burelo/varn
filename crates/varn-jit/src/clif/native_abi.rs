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

pub const NATIVE_FRAMELESS: u64 = 1 << 63;

const MAX_ID_PARAMS: usize = 27;

impl NativeClass {
    fn code(self) -> u64 {
        match self {
            NativeClass::Word => 0,
            NativeClass::Float => 1,
            NativeClass::Boxed => 2,
        }
    }
}

impl NativeShape {
    pub(crate) fn id(&self) -> Option<u64> {
        if self.params.len() > MAX_ID_PARAMS {
            return None;
        }
        let mut id = self.ret.code() | ((self.params.len() as u64) << 2);
        for (i, c) in self.params.iter().enumerate() {
            id |= c.code() << (8 + 2 * i);
        }
        Some(id)
    }
}
