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
use varn_core::{IntrinsicType, ModuleId};
use varn_types::{FunctionProto, NativeCtx};

use crate::linker::Linker;

use super::VmSuspend;

// The JIT helper entry points are named through `ctx::` by `jit::helpers` and
// by the `jit_helper_abi!` list, which knows only bare fn names. A glob rather
// than an explicit list: that list used to be a fourth place every new helper
// had to be written down. A name collision between domain modules is still a
// compile error, so nothing is silently shadowed.
pub(crate) use super::jit_helpers::*;

#[repr(C)]
pub struct ExecCtx {
    pub stack: Vec<VmValue>,
    pub frames: crate::frame_stack::FrameStack,
    pub globals: GlobalStore,
    pub heap: Heap,
    pub try_handlers: Vec<TryHandler>,
    pub modules: FxHashMap<ModuleId, VmValue>,
    pub precompiled: Rc<FxHashMap<ModuleId, Rc<FunctionProto>>>,
    pub loader: Option<std::sync::Arc<dyn ModuleLoader + Send + Sync>>,
    pub settings: crate::settings::ExecSettings,
    pub open_upvalues: Vec<(usize, VmUpvalue)>,
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
    pub proto_constants: FxHashMap<usize, (Rc<FunctionProto>, Rc<Vec<VmValue>>)>,
    pub static_closures: FxHashMap<usize, (Rc<FunctionProto>, VmValue)>,
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
    /// Caller→JIT-prologue frame handshake. Every path that invokes a
    /// `jit_fn` after having pushed the activation's CallFrame itself
    /// (interpreter dispatch, `jit_call`, `jit_invoke_virtual`,
    /// `jit_construct_fast`, the `Call` opcode's asm site after
    /// `jit_prepare_call`) sets this to 1 immediately before the call; a
    /// bare recursive `CallSelf` does not. Every JIT prologue reads and
    /// clears it, pushing its own frame only when entered self-called, so
    /// each activation owns exactly one logical frame.
    pub jit_frame_prepushed: usize,
    /// Post-call resume ip a JIT caller records (via `jit_prepare_call`)
    /// before a fast JIT→JIT call. If the callee (or a deeper frame) throws
    /// and the exception is caught below this caller, its native JIT frame is
    /// unwound by the longjmp; on re-entry the interpreter uses this ip to
    /// resume the caller *interpreted* from just after the call, instead of
    /// re-executing its JIT body from ip=0 (which loops forever). Written on
    /// the fast path where `jit_call` isn't involved.
    pub jit_resume_ip: usize,
    /// Caller destination register for the pending fast JIT→JIT call, written
    /// alongside [`Self::jit_resume_ip`]. `jit_prepare_call` stamps it as the
    /// callee frame's `return_reg` so that, if the callee is later resumed
    /// *interpreted* after an exception unwind, its `Return` writes the result
    /// to the caller's slot (the fast path's machine-return store never ran).
    pub jit_call_dest: usize,
    /// An ON-STACK REPLACEMENT request raised by `OpCode::Loop` when a proto's
    /// back edges crossed the threshold, holding the loop-header ip to resume
    /// at. The opcode cannot service it itself — entering compiled code means
    /// leaving the dispatch loop — so it parks the ip here and returns
    /// `ContinueFrame`; the frame loop takes it on the way round.
    pub osr_request: Option<usize>,
    pub resources: varn_types::ResourceStore,
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
    pub capabilities: varn_types::capabilities::CapabilitySet,
    pub metadata: FxHashMap<String, FxHashMap<String, VmValue>>,
    pub gc_root_scratch: Vec<VmValue>,
}

impl ExecCtx {
    pub(crate) fn new(mut globals: GlobalStore, settings: crate::settings::ExecSettings) -> Self {
        varn_runtime::init_heap();
        let mut heap = Heap::new();

        let fresh = globals.values.is_empty();
        if fresh {
            globals = GlobalStore::with_native_layout(&mut heap);
        }

        let mut ctx = Self {
            stack: Vec::with_capacity(16384),
            frames: crate::frame_stack::FrameStack::with_capacity(512),
            globals,
            heap,
            try_handlers: Vec::new(),
            modules: FxHashMap::default(),
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
            proto_constants: FxHashMap::default(),
            static_closures: FxHashMap::default(),
            linker: Linker::new(),
            jit_jmp_buf: std::ptr::null_mut(),
            jit_suspend_buf: std::ptr::null_mut(),
            jit_panic_exception_handler: None,
            jit_panic_exception_error: None,
            jit_panic_exception_err_obj: None,
            jit_panic_suspend_resume_ip: None,
            jit_native_result: VmValue::null(),
            jit_frame_prepushed: 0,
            jit_resume_ip: 0,
            jit_call_dest: 0,
            osr_request: None,
            resources: varn_types::ResourceStore::new(),
            gc_inhibited: false,
            capabilities: varn_types::capabilities::CapabilitySet::allow_all(),
            metadata: FxHashMap::default(),
            gc_root_scratch: Vec::with_capacity(1024),
        };

        if fresh {
            ctx.init_intrinsics();
            ctx.preload_strings();
        }

        ctx.validate_jit_safepoint_offsets();
        ctx
    }

    /// The JIT back-edge safepoint reads the nursery fill level through raw
    /// offsets (ExecCtx.heap -> RcBox -> HeapInner.nursery.objects.len), and
    /// `emit_nursery_alloc` (`crates/varn-jit/src/clif/nursery.rs`) writes
    /// through two more: the `forwarding` Vec and `Nursery::alloc_count`.
    /// These offsets all bake in Rc/Vec internal layout; verify the whole
    /// chain against the live heap so a std layout change fails loudly at
    /// startup instead of corrupting memory at runtime.
    fn validate_jit_safepoint_offsets(&self) {
        unsafe {
            let base = self as *const ExecCtx as *const u8;
            let rcbox = *(base.add(std::mem::offset_of!(ExecCtx, heap)) as *const *const u8);
            assert_eq!(
                rcbox,
                self.heap.rcbox_ptr_for_validation(),
                "JIT safepoint: ExecCtx.heap does not point at the expected RcBox"
            );
            let len = *(rcbox.add(Heap::nursery_len_byte_offset_from_rcbox()) as *const usize);
            assert_eq!(
                len,
                self.heap.nursery.len(),
                "JIT safepoint: nursery length offset chain is stale"
            );

            // `emit_nursery_alloc` bumps `forwarding`'s length word alongside
            // `objects`'; the two must always agree (`try_alloc` pushes to
            // both together, and `collect` clears both together), so reading
            // it back through the offset chain and comparing against
            // `objects.len()` catches a stale/wrong offset the same way the
            // check above does.
            let fwd_len_off =
                Heap::nursery_fwd_vec_byte_offset_from_rcbox() + 2 * std::mem::size_of::<usize>();
            let fwd_len = *(rcbox.add(fwd_len_off) as *const usize);
            assert_eq!(
                fwd_len,
                self.heap.nursery.len(),
                "JIT allocation: nursery forwarding-vec offset chain is stale"
            );

            // `Nursery::alloc_count` is a plain field, so its raw-read value
            // must match exactly, not just be consistent with another read.
            let alloc_count =
                *(rcbox.add(Heap::nursery_alloc_count_byte_offset_from_rcbox()) as *const u64);
            assert_eq!(
                alloc_count, self.heap.nursery.alloc_count,
                "JIT allocation: nursery alloc_count offset chain is stale"
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
        // dispatch. Names are sourced from the canonical `IntrinsicType` table
        // (no raw literals). This set is broader than the op-id core classes:
        // it includes the `Error` hierarchy but not `Symbol`/`bigint`.
        let names = [
            IntrinsicType::Array.as_str(),
            IntrinsicType::Str.as_str(),
            IntrinsicType::Int.as_str(),
            IntrinsicType::Float.as_str(),
            IntrinsicType::Decimal.as_str(),
            IntrinsicType::Bool.as_str(),
            IntrinsicType::Char.as_str(),
            IntrinsicType::Map.as_str(),
            IntrinsicType::Set.as_str(),
            IntrinsicType::Range.as_str(),
            IntrinsicType::Error.as_str(),
            IntrinsicType::TypeError.as_str(),
            IntrinsicType::RangeError.as_str(),
        ];
        for name in names {
            if let Some(nv) = self.globals.get_by_name(name) {
                if let Some(obj) = self.heap.get(nv.as_heap_idx()) {
                    match obj {
                        crate::heap::HeapObj::Class(cls) => {
                            let cls = cls.clone();
                            self.heap.set_intrinsic_class(name, cls);
                        }
                        crate::heap::HeapObj::NativeFn(f, _) => {
                            let f = *f;
                            if let Ok(class_nv) = (f)(self as &mut dyn NativeCtx, &[]) {
                                if let Some(crate::heap::HeapObj::Class(cls)) =
                                    self.heap.get(class_nv.as_heap_idx())
                                {
                                    let cls = cls.clone();
                                    self.heap.set_intrinsic_class(name, cls);
                                    self.globals.set_by_name(name, class_nv);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub(crate) fn fork_for_task(&self) -> Self {
        Self {
            stack: Vec::with_capacity(1024),
            frames: crate::frame_stack::FrameStack::with_capacity(64),
            globals: self.globals.clone(),
            heap: self.heap.clone(),
            try_handlers: Vec::new(),
            modules: self.modules.clone(),
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
            proto_constants: FxHashMap::default(),
            static_closures: FxHashMap::default(),
            linker: self.linker.clone_state(),
            jit_jmp_buf: std::ptr::null_mut(),
            jit_suspend_buf: std::ptr::null_mut(),
            jit_panic_exception_handler: None,
            jit_panic_exception_error: None,
            jit_panic_exception_err_obj: None,
            jit_panic_suspend_resume_ip: None,
            jit_native_result: VmValue::null(),
            jit_frame_prepushed: 0,
            jit_resume_ip: 0,
            jit_call_dest: 0,
            osr_request: None,
            resources: varn_types::ResourceStore::new(),
            gc_inhibited: false,
            capabilities: self.capabilities.clone(),
            metadata: FxHashMap::default(),
            gc_root_scratch: Vec::with_capacity(1024),
        }
    }

    pub fn run_minor_gc(&mut self) {
        let frame_top = self
            .frames
            .last()
            .map(|f| f.base + f.closure().proto.register_count as usize)
            .unwrap_or(0);
        let active_stack_len = self.stack.len().max(frame_top);
        if self.stack.len() < active_stack_len {
            self.stack.resize(active_stack_len, VmValue::null());
        }

        let mut all_vals = std::mem::take(&mut self.gc_root_scratch);
        all_vals.clear();
        let needed_cap = active_stack_len
            + self.globals.values.len()
            + self.modules.len()
            + self.module_exports.len()
            + self.static_closures.len()
            + self.pending_constructors.len()
            + self.pending_setters.len()
            + 1;
        if all_vals.capacity() < needed_cap {
            all_vals.reserve(needed_cap - all_vals.capacity());
        }

        all_vals.extend_from_slice(&self.stack[..active_stack_len]);
        let globals_start = all_vals.len();
        all_vals.extend_from_slice(&self.globals.values);
        let modules_start = all_vals.len();
        for v in self.modules.values() {
            all_vals.push(*v);
        }
        let module_exports_start = all_vals.len();
        for v in self.module_exports.values() {
            all_vals.push(*v);
        }
        let static_closures_start = all_vals.len();
        for (_, v) in self.static_closures.values() {
            all_vals.push(*v);
        }
        let pending_ctors_start = all_vals.len();
        for (_, v) in &self.pending_constructors {
            all_vals.push(*v);
        }
        let pending_setters_start = all_vals.len();
        for (_, v) in &self.pending_setters {
            all_vals.push(*v);
        }
        let vm_suspend_start = all_vals.len();
        if let Some(VmSuspend::Yield { value, .. }) = &self.vm_suspend {
            all_vals.push(*value);
        }
        let metadata_start = all_vals.len();
        for map in self.metadata.values() {
            for &v in map.values() {
                all_vals.push(v);
            }
        }
        let jit_native_result_start = all_vals.len();
        all_vals.push(self.jit_native_result);

        self.heap.minor_gc(&mut all_vals, &[]);

        self.stack[..active_stack_len].copy_from_slice(&all_vals[..active_stack_len]);

        let globals_slice = &all_vals[globals_start..modules_start];
        self.globals.values.copy_from_slice(globals_slice);

        for (mi, v) in (modules_start..).zip(self.modules.values_mut()) {
            *v = all_vals[mi];
        }

        for (ei, v) in (module_exports_start..).zip(self.module_exports.values_mut()) {
            *v = all_vals[ei];
        }

        for (si, (_, v)) in (static_closures_start..).zip(self.static_closures.values_mut()) {
            *v = all_vals[si];
        }

        for (ci, (_, v)) in (pending_ctors_start..).zip(self.pending_constructors.iter_mut()) {
            *v = all_vals[ci];
        }

        for (sti, (_, v)) in (pending_setters_start..).zip(self.pending_setters.iter_mut()) {
            *v = all_vals[sti];
        }

        if let Some(VmSuspend::Yield { value, .. }) = &mut self.vm_suspend {
            *value = all_vals[vm_suspend_start];
        }

        {
            let mut medi = metadata_start;
            for map in self.metadata.values_mut() {
                for v in map.values_mut() {
                    *v = all_vals[medi];
                    medi += 1;
                }
            }
        }

        self.jit_native_result = all_vals[jit_native_result_start];
        all_vals.clear();
        self.gc_root_scratch = all_vals;
    }

    /// Loop back-edge GC safepoint shared by the interpreter and the JIT.
    ///
    /// Suspended generator state is traced by the collectors themselves
    /// (`scan_and_fix_old_obj`'s Generator arm / the marker's Generator arm),
    /// so async liveness no longer defers collection. Only nested contexts
    /// (`gc_inhibited`) never initiate one — see that field's invariant.
    pub(crate) fn gc_backedge_safepoint(&mut self) {
        if self.gc_inhibited {
            return;
        }
        if self.heap.needs_minor_gc() {
            self.run_minor_gc();
        }
        if self.heap.needs_gc() {
            self.trigger_gc();
        }
    }

    pub(crate) fn trigger_gc(&mut self) {
        if self.gc_inhibited {
            return;
        }
        self.run_minor_gc();
        let frame_top = self
            .frames
            .last()
            .map(|f| f.base + f.closure().proto.register_count as usize)
            .unwrap_or(0);
        let active_stack_len = self.stack.len().max(frame_top);
        if self.stack.len() < active_stack_len {
            self.stack.resize(active_stack_len, VmValue::null());
        }
        let mut roots: Vec<u32> = Vec::with_capacity(256);
        for v in &self.stack[..active_stack_len] {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        if self.jit_native_result.is_heap() {
            roots.push(self.jit_native_result.as_heap_idx());
        }
        for v in &self.globals.values {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }

        for v in self.modules.values() {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        for frame in &self.frames {
            for c in frame.closure().constants.iter() {
                if c.is_heap() {
                    roots.push(c.as_heap_idx());
                }
            }
        }
        for (_, v) in self.static_closures.values() {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        for (_, v) in &self.pending_constructors {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        for (_, v) in &self.pending_setters {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        for map in self.metadata.values() {
            for &v in map.values() {
                if v.is_heap() {
                    roots.push(v.as_heap_idx());
                }
            }
        }
        let _ = self.heap.collect(&roots);
    }
}

pub use crate::arch::{vm_longjmp as my_longjmp, vm_setjmp as my_setjmp, JmpBuf};
