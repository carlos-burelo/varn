use cranelift_codegen::ir::{FuncRef, Value};
use cranelift_codegen::isa::CallConv;
use rustc_hash::{FxHashMap, FxHashSet};
use varn_types::register_meta::FrameLayout;
use varn_types::ssa::SsaProto;
use varn_types::{FunctionProto, VmValue};

use super::super::abi::Activation;
use super::super::piece::CompiledPiece;
use super::call;
use super::views;
use crate::JitHelpers;

pub(super) struct FrameIo {
    pub exec_ctx: Value,
    pub closure: Value,
    pub base: Option<Value>,

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

    pub frame: Option<FrameIo>,
    pub activation: Activation,

    pub this: Option<Value>,

    pub has_round: bool,

    pub views: views::Views,

    pub in_range_steps: FxHashSet<u32>,

    pub no_overflow: FxHashSet<u32>,

    pub fast_bounds: FxHashSet<u32>,

    pub elide: std::cell::Cell<bool>,

    pub carried: FxHashMap<u32, cranelift_frontend::Variable>,

    pub try_live: FxHashSet<u32>,

    pub pure_try: std::cell::RefCell<PureTryState>,

    pub home_addrs: std::cell::RefCell<FxHashMap<u32, Value>>,

    pub natives: std::cell::RefCell<Vec<u64>>,

    pub scratch: Option<call::ScratchWin>,
}

pub(in crate::clif) const NEEDS_ACTIVATION: &str = "from_ssa: body needs a FrameStore activation";

#[derive(Default)]
pub(super) struct PureTryState {
    pub err_ret: Option<cranelift_codegen::ir::Block>,
    pub entry_stacks: Vec<Vec<usize>>,
    pub landing_error: std::collections::BTreeMap<usize, u32>,
    pub clif_map: std::collections::BTreeMap<usize, cranelift_codegen::ir::Block>,
    pub stack: Vec<usize>,
}

pub(in crate::clif) struct Lowered {
    pub piece: CompiledPiece,

    pub natives: Vec<u64>,

    pub frameless: bool,
}
