use crate::closure::VmUpvalue;
use crate::frame::TryHandler;
use crate::globals::GlobalStore;
use crate::heap::Heap;
use crate::loader::ModuleLoader;
use crate::profile::{HotspotCounters, ProfileCounters};
use crate::value::VmValue;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use varn_core::ModuleId;
use varn_types::FunctionProto;

use crate::linker::Linker;

use super::VmSuspend;

type SharedModules = Rc<std::cell::UnsafeCell<FxHashMap<ModuleId, VmValue>>>;
type SharedProtoPools =
    Rc<std::cell::UnsafeCell<FxHashMap<usize, (Rc<FunctionProto>, Rc<Vec<VmValue>>)>>>;
type SharedStaticClosures =
    Rc<std::cell::UnsafeCell<FxHashMap<usize, (Rc<FunctionProto>, VmValue)>>>;
type SharedMetadata = Rc<std::cell::UnsafeCell<FxHashMap<String, FxHashMap<String, VmValue>>>>;
type SharedHashableKeys = Rc<std::cell::UnsafeCell<FxHashMap<(u32, i64), Vec<VmValue>>>>;

#[repr(C)]
pub struct ExecCtx {
    pub stack: crate::frame_store::FrameStore,
    pub frames: crate::frame_stack::FrameStack,
    /// Module-global region, shared across every task fork like the heap:
    /// sibling tasks observe each other's defines in drive order (structured
    /// scope). Same single-threaded `Rc<UnsafeCell>` contract as the heap;
    /// access only through [`Self::globals_ref`] / [`Self::globals_mut`],
    /// never held across nested VM entry.
    pub globals: Rc<std::cell::UnsafeCell<GlobalStore>>,
    pub heap: Heap,
    pub try_handlers: Vec<TryHandler>,
    pub modules: SharedModules,
    pub precompiled: Rc<FxHashMap<ModuleId, Rc<FunctionProto>>>,
    pub loader: Option<std::sync::Arc<dyn ModuleLoader + Send + Sync>>,
    pub settings: crate::settings::ExecSettings,
    pub open_upvalues: Vec<(crate::frame_store::SlotAddr, VmUpvalue)>,
    pub pending_constructors: Vec<(usize, VmValue)>,
    pub pending_setters: Vec<(usize, VmValue)>,
    pub vm_suspend: Option<VmSuspend>,
    pub module_exports: FxHashMap<usize, VmValue>,
    pub opcode_counts: Option<Rc<Vec<std::sync::atomic::AtomicU64>>>,
    pub profile_counters: Option<Arc<ProfileCounters>>,
    pub hotspot_counters: Option<Rc<RefCell<HotspotCounters>>>,
    /// Both of these are keyed by `Rc::as_ptr(&proto)`, and each entry holds a
    /// strong ref to the proto that address belongs to. That ref is not
    /// decoration: without it the `Rc` can be dropped while the entry lives on,
    /// the allocator can hand the same address to a DIFFERENT proto, and the
    /// cache then answers with another function's constant pool — a silent
    /// miscompile ("a" + `<object>` + "b" where a literal should be). Holding
    /// the proto makes the address ours for as long as it is a key.
    pub proto_constants: SharedProtoPools,
    pub static_closures: SharedStaticClosures,
    pub linker: Linker,
    pub jit_jmp_buf: *mut JmpBuf,
    /// The OUTERMOST clif frame's jump buffer. `jit_jmp_buf` is per-frame so
    /// a throw unwinds one clif frame at a time, but a suspension cannot do
    /// that — an intermediate clif frame has no way to park in the middle of
    /// a native function — so `jit_await`/`jit_yield` jump here instead.
    pub jit_suspend_buf: *mut JmpBuf,
    pub jit_panic_exception_handler: Option<crate::frame::TryHandler>,
    pub jit_panic_exception_error: Option<VmValue>,
    pub jit_panic_exception_err_obj: Option<crate::error::RuntimeError>,
    pub jit_panic_suspend_resume_ip: Option<usize>,
    pub jit_native_result: VmValue,
    pub jit_exit: varn_jit::stack_roots::JitExit,
    pub jit_exits_saved: Vec<varn_jit::stack_roots::JitExit>,
    /// An ON-STACK REPLACEMENT request raised by `OpCode::Loop` when a proto's
    /// back edges crossed the threshold, holding the loop-header ip to resume
    /// at. The opcode cannot service it itself — entering compiled code means
    /// leaving the dispatch loop — so it parks the ip here and returns
    /// `ContinueFrame`; the frame loop takes it on the way round.
    pub osr_request: Option<usize>,
    /// Process-global resource table, shared across every task fork like the
    /// heap: resource ids are unique per process, so a fork-local copy would
    /// alias live ids and drop handles created inside tasks. Same
    /// single-threaded `Rc<UnsafeCell>` contract as [`Heap`](crate::heap::Heap).
    pub resources: Rc<std::cell::UnsafeCell<varn_types::ResourceStore>>,
    /// Nested contexts (sync-generator bodies) share the heap but own a
    /// private stack the outer context's GC roots cannot see in the other
    /// direction: a collection triggered from *inside* the nested context
    /// would never root the suspended outer stack. Such contexts must never
    /// initiate a collection — allocation overflows to the old gen when the
    /// nursery is full, and the owning context collects (tracing the
    /// generator's saved state via `GeneratorDriver::trace_vm_values_mut`)
    /// at its next safepoint. Keep this LAST: `ExecCtx` is `#[repr(C)]` and
    /// JIT code addresses leading fields by raw offset.
    pub gc_inhibited: bool,
    pub capabilities: Rc<varn_types::capabilities::CapabilitySet>,
    pub metadata: SharedMetadata,
    /// Map/Set key representatives of `Hashable & Equatable` instances, by
    /// `(class id, hash())` (see `hashable_keys.rs`). GC roots.
    pub(crate) hashable_keys: SharedHashableKeys,
    /// Staging para el protocolo lento de llamadas (P2): la ventana
    /// callee+args como `Vec<VmValue>` contiguo, reutilizado entre llamadas.
    /// `prepare_call` la consume de forma síncrona (adopta al frame o la
    /// drena), así que nunca hay staging anidado vivo a la vez.
    pub stage: Vec<VmValue>,
    /// Cola del scheduler cooperativo: forks listos y parqueados que este
    /// contexto drivea. Va al final junto a `stage`: ningún código JIT
    /// direcciona estos campos por offset crudo.
    pub(crate) task_queue: std::cell::OnceCell<super::scheduler::TaskQueue>,
}

impl ExecCtx {
    pub(crate) fn new(mut globals: GlobalStore, settings: crate::settings::ExecSettings) -> Self {
        let mut heap = Heap::new();

        let fresh = globals.values.is_empty();
        if fresh {
            globals = GlobalStore::with_native_layout(&mut heap);
        }

        let mut ctx = Self {
            stack: crate::frame_store::FrameStore::new(),
            frames: crate::frame_stack::FrameStack::with_capacity(512),
            globals: Rc::new(std::cell::UnsafeCell::new(globals)),
            heap,
            try_handlers: Vec::new(),
            modules: Rc::new(std::cell::UnsafeCell::new(FxHashMap::default())),
            precompiled: Rc::new(FxHashMap::default()),
            loader: None,
            settings,
            open_upvalues: Vec::new(),
            pending_constructors: Vec::new(),
            pending_setters: Vec::new(),
            vm_suspend: None,
            module_exports: FxHashMap::default(),
            opcode_counts: None,
            profile_counters: None,
            hotspot_counters: None,
            proto_constants: Rc::new(std::cell::UnsafeCell::new(FxHashMap::default())),
            static_closures: Rc::new(std::cell::UnsafeCell::new(FxHashMap::default())),
            linker: Linker::new(),
            jit_jmp_buf: std::ptr::null_mut(),
            jit_suspend_buf: std::ptr::null_mut(),
            jit_panic_exception_handler: None,
            jit_panic_exception_error: None,
            jit_panic_exception_err_obj: None,
            jit_panic_suspend_resume_ip: None,
            jit_exit: varn_jit::stack_roots::JitExit::default(),
            jit_exits_saved: Vec::new(),
            jit_native_result: VmValue::null(),
            osr_request: None,
            resources: Rc::new(std::cell::UnsafeCell::new(varn_types::ResourceStore::new())),
            gc_inhibited: false,
            capabilities: Rc::new(varn_types::capabilities::CapabilitySet::allow_all()),
            metadata: Rc::new(std::cell::UnsafeCell::new(FxHashMap::default())),
            hashable_keys: Rc::new(std::cell::UnsafeCell::new(FxHashMap::default())),
            stage: Vec::with_capacity(32),
            task_queue: std::cell::OnceCell::new(),
        };

        if fresh {
            ctx.init_intrinsics();
            ctx.preload_strings();
        }

        ctx.validate_jit_safepoint_offsets();
        ctx
    }

    /// The JIT back-edge safepoint reads the young-birth count through raw
    /// offsets (ExecCtx.heap -> RcBox -> HeapInner.young.born.len).
    /// They bake in Rc/Vec internal layout; verify the chain against the live
    /// heap so a std layout change fails loudly at startup instead of
    /// corrupting memory at runtime.
    fn validate_jit_safepoint_offsets(&self) {
        unsafe {
            let base = self as *const ExecCtx as *const u8;
            let rcbox = *(base.add(std::mem::offset_of!(ExecCtx, heap)) as *const *const u8);
            assert_eq!(
                rcbox,
                self.heap.rcbox_ptr_for_validation(),
                "JIT safepoint: ExecCtx.heap does not point at the expected RcBox"
            );
            let len = *(rcbox.add(Heap::young_len_byte_offset_from_rcbox()) as *const usize);
            assert_eq!(
                len,
                self.heap.young.len(),
                "JIT safepoint: young length offset chain is stale"
            );
        }
    }

    fn preload_strings(&mut self) {
        const COMMON_STRINGS: &[&str] = &[
            "PASSED",
            "FAILED",
            "error",
            "message",
            "value",
            "result",
            "length",
            "name",
            "type",
            "ok",
            "err",
            "true",
            "false",
            "toString",
            "valueOf",
            "constructor",
        ];
        for s in COMMON_STRINGS {
            self.heap.alloc_str_interned(s);
        }
    }

    fn init_intrinsics(&mut self) {
        // Intrinsic classes the VM registers for property/method fallback
        // dispatch. Names are sourced from the canonical `RuntimeKind` names
        // (no raw literals). This set is broader than the op-id core classes:
        // it includes the `Error` hierarchy but not `Symbol`/`bigint`.
        let names = [
            varn_core::RuntimeKind::Array.name(),
            varn_core::RuntimeKind::Str.name(),
            varn_core::RuntimeKind::Int.name(),
            varn_core::RuntimeKind::Float.name(),
            varn_core::RuntimeKind::Decimal.name(),
            varn_core::RuntimeKind::Bool.name(),
            varn_core::RuntimeKind::Char.name(),
            varn_core::RuntimeKind::Map.name(),
            varn_core::RuntimeKind::Set.name(),
            varn_core::RuntimeKind::Range.name(),
            varn_core::well_known::ERROR,
            varn_core::well_known::TYPE_ERROR,
            varn_core::well_known::RANGE_ERROR,
            varn_core::RuntimeErrorKind::IntegerOverflow.class_name(),
            varn_core::RuntimeErrorKind::DivisionByZero.class_name(),
            varn_core::RuntimeErrorKind::MatchError.class_name(),
        ];
        for name in names {
            if let Some(nv) = self.globals_ref().get_by_name(name) {
                if let Some(obj) = self.heap.get(nv.as_heap()) {
                    match obj {
                        crate::heap::HeapObj::Class(cls) => {
                            let cls = cls.clone();
                            self.heap.set_intrinsic_class(name, cls);
                        }
                        crate::heap::HeapObj::NativeFn(f, _) => {
                            let f = *f;
                            if let Ok(class_nv) = self.invoke_native(f, &[]) {
                                if let Some(crate::heap::HeapObj::Class(cls)) =
                                    self.heap.get(class_nv.as_heap())
                                {
                                    let cls = cls.clone();
                                    self.heap.set_intrinsic_class(name, cls);
                                    self.globals_mut().set_by_name(name, class_nv);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    #[allow(clippy::mut_from_ref)]
    pub(crate) fn globals_ref(&self) -> &GlobalStore {
        unsafe { &*self.globals.get() }
    }

    /// `&mut` out of the shared table under the same single-threaded
    /// contract as the heap: tasks on this thread take turns, never
    /// overlapping, so no aliasing `&mut` exists at once. Never hold the
    /// result across nested VM entry (`call_vm`/`invoke`).
    #[allow(clippy::mut_from_ref)]
    pub(crate) fn globals_mut(&mut self) -> &mut GlobalStore {
        unsafe { &mut *self.globals.get() }
    }

    #[allow(clippy::mut_from_ref)]
    pub(crate) fn modules_ref(&self) -> &FxHashMap<ModuleId, VmValue> {
        unsafe { &*self.modules.get() }
    }

    pub(crate) fn fork_for_task(&self) -> Self {
        Self {
            stack: crate::frame_store::FrameStore::new_for_task(),
            frames: crate::frame_stack::FrameStack::with_capacity(8),
            globals: Rc::clone(&self.globals),
            heap: self.heap.clone(),
            try_handlers: Vec::new(),
            modules: Rc::clone(&self.modules),
            precompiled: Rc::clone(&self.precompiled),
            loader: self.loader.clone(),
            settings: self.settings,
            open_upvalues: Vec::new(),
            pending_constructors: Vec::new(),
            pending_setters: Vec::new(),
            vm_suspend: None,
            module_exports: FxHashMap::default(),
            opcode_counts: None,
            profile_counters: None,
            hotspot_counters: None,
            proto_constants: Rc::clone(&self.proto_constants),
            static_closures: Rc::clone(&self.static_closures),
            linker: self.linker.share(),
            jit_jmp_buf: std::ptr::null_mut(),
            jit_suspend_buf: std::ptr::null_mut(),
            jit_panic_exception_handler: None,
            jit_panic_exception_error: None,
            jit_panic_exception_err_obj: None,
            jit_panic_suspend_resume_ip: None,
            jit_exit: varn_jit::stack_roots::JitExit::default(),
            jit_exits_saved: Vec::new(),
            jit_native_result: VmValue::null(),
            osr_request: None,
            resources: Rc::clone(&self.resources),
            gc_inhibited: false,
            capabilities: Rc::clone(&self.capabilities),
            metadata: Rc::clone(&self.metadata),
            hashable_keys: Rc::clone(&self.hashable_keys),
            stage: Vec::new(),
            task_queue: std::cell::OnceCell::new(),
        }
    }
}

pub use crate::arch::{vm_longjmp as my_longjmp, vm_setjmp as my_setjmp, JmpBuf};
