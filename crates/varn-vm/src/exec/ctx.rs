use crate::frame::{CallFrame, TryHandler, VmUpvalue};
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
use varn_types::value::LazyTask;
use varn_types::{FunctionProto, NativeCtx};

use crate::linker::Linker;

use super::VmSuspend;
use varn_types::generator::GenChannel;

pub use super::ctx_jit_runtime::{
    jit_array_extend, jit_array_get_fast, jit_array_length, jit_array_pop, jit_array_push,
    jit_array_set_fast, jit_assert_not_null, jit_await, jit_bind_method, jit_bitand, jit_bitor,
    jit_bitxor, jit_build_object, jit_build_object_with_shape, jit_build_record_with_shape, jit_call_spread,
    jit_class_member_op, jit_close_upvalue, clif_call_fallback, jit_declare_field,
    jit_define_global, jit_div,
    jit_gc_safepoint, jit_get_enum_tag, jit_get_fixed_field, jit_get_index,
    jit_get_property_maybe_stub, jit_get_super, jit_get_symbol, jit_inherit, jit_instanceof,
    jit_is_array_stub, jit_load_module, jit_load_module_by_idx, jit_load_module_slot,
    jit_logical_not, jit_make_class, jit_make_enum_variant, jit_modulo, jit_negate,
    jit_object_keys_stub, jit_object_merge_stub, jit_object_rest, jit_op_in_stub, jit_pop_try,
    jit_pow, jit_push_try, jit_range, jit_set_fixed_field, jit_set_index, jit_shl, jit_shr,
    jit_spawn, jit_store_global, jit_store_module_slot, jit_str_concat, jit_str_length,
    jit_str_slice, jit_throw, jit_typeof_val, jit_ushr, jit_wrap_spread_stub, jit_yield,
};
pub use super::ctx_jit_values::{
    jit_add, jit_build_array, jit_build_str, jit_call, jit_call_method, jit_call_method_flat,
    jit_call_native_fast, jit_call_native_fnptr, jit_call_native_op, jit_define_global_idx,
    jit_dispatch_intrinsic, jit_ensure_stack_capacity, jit_eq, jit_get_property,
    jit_get_property_flat, jit_get_property_ic_fast, jit_get_property_maybe_ic_fast, jit_gt,
    jit_gte, jit_invoke_virtual, jit_invoke_virtual_flat, jit_is_native_fn, jit_load_const,
    jit_load_global, jit_load_global_idx, jit_load_static_fn, jit_load_upvalue, jit_lt, jit_lte,
    jit_make_closure, jit_mul, jit_neq, jit_post_call, jit_prepare_call, jit_push_self_frame,
    jit_set_property, jit_set_property_flat, jit_store_global_idx, jit_store_upvalue,
    jit_str_char_code_at, jit_str_char_code_at_fast, jit_str_slice_intrinsic,
    jit_str_substring_intrinsic, jit_sub,
    jit_to_string,
};

#[repr(C)]
pub struct ExecCtx {
    pub stack: Vec<VmValue>,
    pub frames: Vec<CallFrame>,
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
    pub gen_channel: Option<Rc<GenChannel>>,
    pub deferred_tasks: FxHashMap<usize, Rc<LazyTask>>,
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
}

impl ExecCtx {
    pub fn new(mut globals: GlobalStore, settings: crate::settings::ExecSettings) -> Self {
        varn_runtime::init_heap();
        let mut heap = Heap::new();

        let fresh = globals.values.is_empty();
        if fresh {
            globals = GlobalStore::with_native_layout(&mut heap);
        }

        let mut ctx = Self {
            stack: Vec::with_capacity(16384),
            frames: Vec::with_capacity(512),
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
            gen_channel: None,
            deferred_tasks: FxHashMap::default(),
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
                        crate::heap::HeapObj::NativeFn(_, f) => {
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

    pub fn fork_for_task(&self) -> Self {
        Self {
            stack: Vec::with_capacity(1024),
            frames: Vec::with_capacity(64),
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
            gen_channel: None,
            deferred_tasks: FxHashMap::default(),
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
        }
    }

    pub fn run_minor_gc(&mut self) {
        let stack_len = self.stack.len();

        let mut all_vals: Vec<VmValue> = Vec::with_capacity(
            stack_len
                + self.globals.values.len()
                + self.modules.len()
                + self.module_exports.len()
                + self.static_closures.len()
                + self.pending_constructors.len()
                + self.pending_setters.len(),
        );

        all_vals.extend_from_slice(&self.stack);
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

        self.heap.minor_gc(&mut all_vals, &[]);

        self.stack.copy_from_slice(&all_vals[..stack_len]);

        let globals_slice = &all_vals[globals_start..modules_start];
        self.globals.values.copy_from_slice(globals_slice);

        {
            let mut mi = modules_start;
            for v in self.modules.values_mut() {
                *v = all_vals[mi];
                mi += 1;
            }
        }

        {
            let mut ei = module_exports_start;
            for v in self.module_exports.values_mut() {
                *v = all_vals[ei];
                ei += 1;
            }
        }

        {
            let mut si = static_closures_start;
            for (_, v) in self.static_closures.values_mut() {
                *v = all_vals[si];
                si += 1;
            }
        }

        {
            let mut ci = pending_ctors_start;
            for (_, v) in self.pending_constructors.iter_mut() {
                *v = all_vals[ci];
                ci += 1;
            }
        }

        {
            let mut sti = pending_setters_start;
            for (_, v) in self.pending_setters.iter_mut() {
                *v = all_vals[sti];
                sti += 1;
            }
        }

        if let Some(VmSuspend::Yield { value, .. }) = &mut self.vm_suspend {
            *value = all_vals[vm_suspend_start];
        }
    }

    /// Loop back-edge GC safepoint shared by the interpreter and the JIT.
    ///
    /// Suspended generator state is traced by the collectors themselves
    /// (`scan_and_fix_old_obj`'s Generator arm / the marker's Generator arm),
    /// so async liveness no longer defers collection. Only nested contexts
    /// (`gc_inhibited`) never initiate one — see that field's invariant.
    pub fn gc_backedge_safepoint(&mut self) {
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

    pub fn trigger_gc(&mut self) {
        if self.gc_inhibited {
            return;
        }
        self.run_minor_gc();
        let mut roots: Vec<u32> = Vec::with_capacity(256);
        for v in &self.stack {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        for v in &self.globals.values {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }

        for (_k, v) in &self.modules {
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
        let _ = self.heap.collect(&roots);
    }
}

#[derive(Default, Debug, Clone, Copy)]
#[repr(C)]
pub struct JmpBuf {
    pub rdi: u64,
    pub rsi: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rsp: u64,
    pub rip: u64,
}

#[unsafe(naked)]
pub unsafe extern "C" fn my_setjmp(_buf: *mut JmpBuf) -> i32 {
    std::arch::naked_asm!(
        "mov [rcx + 0],  rdi",
        "mov [rcx + 8],  rsi",
        "mov [rcx + 16], rbx",
        "mov [rcx + 24], rbp",
        "mov [rcx + 32], r12",
        "mov [rcx + 40], r13",
        "mov [rcx + 48], r14",
        "mov [rcx + 56], r15",
        "lea r10, [rsp + 8]",
        "mov [rcx + 64], r10",
        "mov r10, [rsp]",
        "mov [rcx + 72], r10",
        "xor eax, eax",
        "ret"
    );
}

#[unsafe(naked)]
pub unsafe extern "C" fn my_longjmp(_buf: *const JmpBuf, _val: i32) -> ! {
    std::arch::naked_asm!(
        "mov rdi, [rcx + 0]",
        "mov rsi, [rcx + 8]",
        "mov rbx, [rcx + 16]",
        "mov rbp, [rcx + 24]",
        "mov r12, [rcx + 32]",
        "mov r13, [rcx + 40]",
        "mov r14, [rcx + 48]",
        "mov r15, [rcx + 56]",
        "mov rsp, [rcx + 64]",
        "mov eax, edx",
        "jmp qword ptr [rcx + 72]"
    );
}
