use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use super::super::inline_cache::{FeedbackVector, PolyICSlot};
use super::super::literal::opt_rc_str_serde;
use super::super::Chunk;

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ImportLayout {
    pub source: Arc<str>,
    pub slots: Vec<(Arc<str>, u16)>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct FunctionProto {
    #[serde(with = "opt_rc_str_serde")]
    pub name: Option<Arc<str>>,
    pub arity: usize,

    pub export_names: Vec<Arc<str>>,

    #[serde(default)]
    pub import_layouts: Vec<ImportLayout>,

    pub register_count: u16,
    pub has_rest: bool,
    #[serde(default)]
    pub checks_params: bool,
    pub is_async: bool,
    pub is_generator: bool,
    pub has_this: bool,
    pub upvalue_count: usize,
    pub cache_count: usize,
    pub chunk: Chunk,

    #[serde(default)]
    pub required_caps: Vec<std::sync::Arc<str>>,

    #[serde(default)]
    pub state_size: u16,

    #[serde(default)]
    pub global_count: u32,

    #[serde(default)]
    pub register_meta: Vec<crate::register_meta::RegisterMeta>,

    #[serde(default)]
    pub exception_table: Vec<super::records::ExceptionRange>,

    #[serde(default)]
    pub param_kinds: Vec<crate::register_meta::SlotKind>,

    #[serde(default = "slot_kind_dynamic")]
    pub return_kind: crate::register_meta::SlotKind,

    #[serde(skip, default)]
    pub resolved_shapes: RefCell<Vec<(u32, Rc<crate::Shape>)>>,

    #[serde(skip)]
    #[serde(default)]
    pub jit_entry: std::cell::Cell<usize>,

    #[serde(skip)]
    #[serde(default)]
    pub jit_native: std::cell::Cell<usize>,

    #[serde(skip)]
    #[serde(default)]
    pub jit_native_sig: std::cell::Cell<u64>,

    #[serde(skip)]
    #[serde(default)]
    pub jit_code: std::cell::RefCell<Option<Rc<dyn std::any::Any>>>,

    #[serde(skip)]
    #[serde(default)]
    pub jit_failed: std::cell::Cell<bool>,

    #[serde(skip)]
    #[serde(default)]
    pub jit_epoch: std::cell::Cell<u64>,

    #[serde(skip)]
    #[serde(default)]
    pub backedge_memo: std::cell::Cell<u8>,

    #[serde(skip, default = "proto_ic_default")]
    pub ic_cache: Rc<RefCell<Vec<PolyICSlot>>>,

    #[serde(skip, default = "proto_feedback_default")]
    pub feedback: Rc<RefCell<FeedbackVector>>,

    #[serde(skip)]
    #[serde(default)]
    pub frame_layout: std::cell::OnceCell<Rc<crate::register_meta::FrameLayout>>,

    #[serde(skip)]
    #[serde(default)]
    pub static_closure_val: std::cell::Cell<u64>,

    #[serde(skip)]
    #[serde(default)]
    pub jit_entry_count: std::cell::Cell<u32>,

    #[serde(skip)]
    #[serde(default)]
    pub backedge_count: std::cell::Cell<u32>,

    #[serde(skip)]
    #[serde(default)]
    pub jit_osr_entries: std::cell::RefCell<Vec<OsrCacheEntry>>,

    #[serde(default)]
    pub ssa: crate::ssa::PortableSsa,

    #[serde(default)]
    pub suspend_live: Vec<super::records::SuspendLive>,

    #[serde(default)]
    pub param_contract: Option<super::records::ParamContract>,

    #[serde(skip)]
    #[serde(default)]
    pub param_types: std::cell::OnceCell<Option<varn_core::TypeDesc>>,
}

fn slot_kind_dynamic() -> crate::register_meta::SlotKind {
    crate::register_meta::SlotKind::Dynamic
}

#[derive(Clone, Debug)]
pub struct OsrCacheEntry {
    pub ip: usize,
    pub mode: crate::ssa::OsrMode,
    pub epoch: u64,
    pub entry: Option<usize>,
    pub code: Option<Rc<dyn std::any::Any>>,
}

fn proto_ic_default() -> Rc<RefCell<Vec<PolyICSlot>>> {
    Rc::new(RefCell::new(Vec::new()))
}

fn proto_feedback_default() -> Rc<RefCell<FeedbackVector>> {
    Rc::new(RefCell::new(FeedbackVector::default()))
}
