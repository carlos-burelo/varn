use cranelift_codegen::ir::{FuncRef, Value};
use cranelift_codegen::isa::CallConv;
use rustc_hash::{FxHashMap, FxHashSet};
use varn_types::register_meta::FrameLayout;
use varn_types::ssa::SsaProto;
use varn_types::{FunctionProto, VmValue};

use super::super::abi::Activation;
use super::super::lower::ClifLinker;
use super::super::piece::CompiledPiece;
use super::call;
use super::views;
use crate::JitHelpers;

pub(super) struct FrameIo<'a> {
    pub exec_ctx: Value,
    pub closure: Value,
    pub base: Option<Value>,
    pub linker: &'a dyn ClifLinker,

    pub layout: FrameLayout,
}

pub(super) struct Ctx<'a> {
    pub cc: CallConv,
    pub helpers: &'a JitHelpers,
    pub ssa: &'a SsaProto,
    pub proto: &'a FunctionProto,

    pub constants: &'a [VmValue],
    pub self_ref: FuncRef,

    pub exec_ctx: Value,

    pub frame: Option<FrameIo<'a>>,
    pub activation: Activation,

    pub this: Option<Value>,

    pub has_round: bool,

    pub views: views::Views,

    pub in_range_steps: FxHashSet<u32>,

    pub carried: FxHashMap<u32, cranelift_frontend::Variable>,

    pub home_addrs: std::cell::RefCell<FxHashMap<u32, Value>>,

    pub scratch: Option<call::ScratchWin>,
}

pub(in crate::clif) const NEEDS_ACTIVATION: &str = "from_ssa: body needs a FrameStore activation";

pub(in crate::clif) struct Lowered {
    pub piece: CompiledPiece,

    pub frameless: bool,
}
