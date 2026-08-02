use crate::exec::ctx;
use crate::value::VmValue;
use std::cell::RefCell;
use std::rc::Rc;
use varn_base::VmValuePayload;
use varn_types::chunk::PolyICSlot;
use varn_types::FunctionProto;
pub use varn_types::VmValueRef;

#[derive(Debug, Clone)]
pub struct VmUpvalue {
    pub inner: Rc<RefCell<VmUpvalueInner>>,
}

#[derive(Debug, Clone)]
pub struct VmUpvalueInner {
    pub value: VmValue,
    pub stack_slot: Option<usize>,
}

impl VmUpvalue {
    pub fn open(stack_slot: usize) -> Self {
        Self {
            inner: Rc::new(RefCell::new(VmUpvalueInner {
                value: VmValue::null(),
                stack_slot: Some(stack_slot),
            })),
        }
    }

    pub fn closed(value: VmValue) -> Self {
        Self {
            inner: Rc::new(RefCell::new(VmUpvalueInner {
                value,
                stack_slot: None,
            })),
        }
    }

    pub fn read(&self, stack: &[VmValue]) -> VmValue {
        let g = self.inner.borrow_mut();
        match g.stack_slot {
            Some(slot) => stack[slot],
            None => g.value,
        }
    }

    pub fn write(&self, val: VmValue, stack: &mut Vec<VmValue>) {
        let mut g = self.inner.borrow_mut();
        match g.stack_slot {
            Some(slot) => stack[slot] = val,
            None => g.value = val,
        }
    }

    pub fn close(&self, stack: &[VmValue]) {
        let mut g = self.inner.borrow_mut();
        if let Some(slot) = g.stack_slot.take() {
            g.value = stack[slot];
        }
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct VmClosure {
    pub proto: Rc<FunctionProto>,
    pub upvalues: Vec<VmUpvalue>,
    pub constants: Rc<Vec<VmValue>>,
    pub ic_cache: Rc<RefCell<Vec<PolyICSlot>>>,
    pub feedback: Rc<RefCell<varn_types::chunk::FeedbackVector>>,
}

/// Build the production `JitHelpers` table. All fields are static (function
/// addresses + host-struct offsets + probed layouts) — no live `ExecCtx`
/// needed — so both `compile_jit` and the `vn debug -p clif` inspection path
/// share this single source of truth.
pub fn build_jit_helpers() -> varn_jit::JitHelpers {
    let array_layout = crate::heap::Heap::jit_array_layout();
    // `ExecCtx.stack` is a BARE `Vec<VmValue>`, so its data-pointer word is at
    // the bare-`Vec` ptr offset — NOT `elems_ptr_off`, which since the
    // `ArrayRepr` wrapping is measured relative to the `ArrayRepr` (tag +
    // padding + `Vec`) and so includes the wrapper. `slots_ptr_off` is that
    // bare offset (a `Vec`'s field layout is element-type-independent, so the
    // `Vec<Option<HeapObj>>` probe yields the same ptr offset as a
    // `Vec<VmValue>`).
    let stack_data_offset =
        std::mem::offset_of!(ctx::ExecCtx, stack) + array_layout.slots_ptr_off;
    varn_jit::JitHelpers {
        load_const: ctx::jit_load_const as usize,
        load_global_idx: ctx::jit_load_global_idx as usize,
        store_global_idx: ctx::jit_store_global_idx as usize,
        define_global_idx: ctx::jit_define_global_idx as usize,
        eq: ctx::jit_eq as usize,
        neq: ctx::jit_neq as usize,
        lt: ctx::jit_lt as usize,
        lte: ctx::jit_lte as usize,
        gt: ctx::jit_gt as usize,
        gte: ctx::jit_gte as usize,
        add: ctx::jit_add as usize,
        sub: ctx::jit_sub as usize,
        mul: ctx::jit_mul as usize,
        div: ctx::jit_div as usize,
        modulo: ctx::jit_modulo as usize,
        pow: ctx::jit_pow as usize,
        to_string: ctx::jit_to_string as usize,
        load_global: ctx::jit_load_global as usize,
        load_upvalue: ctx::jit_load_upvalue as usize,
        store_upvalue: ctx::jit_store_upvalue as usize,
        make_closure: ctx::jit_make_closure as usize,
        load_static_fn: ctx::jit_load_static_fn as usize,
        call: ctx::jit_call as usize,
        call_method: ctx::jit_call_method as usize,
        call_method_flat: ctx::jit_call_method_flat as usize,
        invoke_virtual_flat: ctx::jit_invoke_virtual_flat as usize,
        get_property: ctx::jit_get_property as usize,
        get_property_flat: ctx::jit_get_property_flat as usize,
        set_property: ctx::jit_set_property as usize,
        set_property_flat: ctx::jit_set_property_flat as usize,
        build_array: ctx::jit_build_array as usize,
        build_str: ctx::jit_build_str as usize,
        negate: ctx::jit_negate as usize,
        logical_not: ctx::jit_logical_not as usize,
        get_index: ctx::jit_get_index as usize,
        set_index: ctx::jit_set_index as usize,
        jit_array_get_fast: ctx::jit_array_get_fast as usize,
        jit_array_set_fast: ctx::jit_array_set_fast as usize,
        typeof_val: ctx::jit_typeof_val as usize,
        instanceof: ctx::jit_instanceof as usize,
        array_length: ctx::jit_array_length as usize,
        array_push: ctx::jit_array_push as usize,
        array_pop: ctx::jit_array_pop as usize,
        array_extend: ctx::jit_array_extend as usize,
        str_concat: ctx::jit_str_concat as usize,
        str_slice: ctx::jit_str_slice as usize,
        str_length: ctx::jit_str_length as usize,
        bit_and: ctx::jit_bitand as usize,
        bit_or: ctx::jit_bitor as usize,
        bit_xor: ctx::jit_bitxor as usize,
        shl: ctx::jit_shl as usize,
        shr: ctx::jit_shr as usize,
        ushr: ctx::jit_ushr as usize,
        load_module: ctx::jit_load_module as usize,
        load_module_slot: ctx::jit_load_module_slot as usize,
        store_module_slot: ctx::jit_store_module_slot as usize,
        build_object_with_shape: ctx::jit_build_object_with_shape as usize,
        range: ctx::jit_range as usize,
        assert_not_null: ctx::jit_assert_not_null as usize,
        close_upvalue: ctx::jit_close_upvalue as usize,
        get_enum_tag: ctx::jit_get_enum_tag as usize,
        is_array: ctx::jit_is_array_stub as usize,
        wrap_spread: ctx::jit_wrap_spread_stub as usize,
        object_keys: ctx::jit_object_keys_stub as usize,
        op_in: ctx::jit_op_in_stub as usize,
        object_merge: ctx::jit_object_merge_stub as usize,
        get_fixed_field: ctx::jit_get_fixed_field as usize,
        set_fixed_field: ctx::jit_set_fixed_field as usize,
        get_property_maybe: ctx::jit_get_property_maybe_stub as usize,
        get_super: ctx::jit_get_super as usize,
        get_symbol: ctx::jit_get_symbol as usize,
        bind_method: ctx::jit_bind_method as usize,
        define_global: ctx::jit_define_global as usize,
        store_global: ctx::jit_store_global as usize,
        declare_field: ctx::jit_declare_field as usize,
        make_class: ctx::jit_make_class as usize,
        inherit: ctx::jit_inherit as usize,
        class_member_op: ctx::jit_class_member_op as usize,
        build_object: ctx::jit_build_object as usize,
        object_rest: ctx::jit_object_rest as usize,
        make_enum_variant: ctx::jit_make_enum_variant as usize,
        spawn: ctx::jit_spawn as usize,
        call_spread: ctx::jit_call_spread as usize,
        load_module_by_idx: ctx::jit_load_module_by_idx as usize,
        invoke_virtual: ctx::jit_invoke_virtual as usize,
        try_push: ctx::jit_push_try as usize,
        try_pop: ctx::jit_pop_try as usize,
        throw: ctx::jit_throw as usize,
        await_helper: ctx::jit_await as usize,
        yield_helper: ctx::jit_yield as usize,
        get_property_ic_fast: ctx::jit_get_property_ic_fast as usize,
        get_property_maybe_ic_fast: ctx::jit_get_property_maybe_ic_fast as usize,
        jit_prepare_call: ctx::jit_prepare_call as usize,
        jit_push_self_frame: ctx::jit_push_self_frame as usize,
        jit_post_call: ctx::jit_post_call as usize,
        jit_ensure_stack_capacity: ctx::jit_ensure_stack_capacity as usize,
        dispatch_intrinsic: ctx::jit_dispatch_intrinsic as usize,
        jit_is_native_fn: ctx::jit_is_native_fn as usize,
        jit_call_native_fast: ctx::jit_call_native_fast as usize,
        jit_call_native_op: ctx::jit_call_native_op as usize,
        jit_call_native_fnptr: ctx::jit_call_native_fnptr as usize,
        resolve_native_op: VmClosure::resolve_native_op_addr,
        resolve_native_op_v2: VmClosure::resolve_native_op_addr_v2,
        array_layout,
        object_layout: crate::heap::Heap::jit_object_layout(),
        str_layout: crate::heap::Heap::jit_str_layout(),
        open_upvalues_offset: {
            let dummy = std::mem::MaybeUninit::<ctx::ExecCtx>::uninit();
            let dummy_ptr = dummy.as_ptr();
            unsafe {
                (std::ptr::addr_of!((*dummy_ptr).open_upvalues) as usize) - (dummy_ptr as usize)
            }
        },
        pending_constructors_offset: {
            let dummy = std::mem::MaybeUninit::<ctx::ExecCtx>::uninit();
            let dummy_ptr = dummy.as_ptr();
            unsafe {
                (std::ptr::addr_of!((*dummy_ptr).pending_constructors) as usize)
                    - (dummy_ptr as usize)
            }
        },
        gc_safepoint: ctx::jit_gc_safepoint as usize,
        heap_field_offset: std::mem::offset_of!(ctx::ExecCtx, heap),
        nursery_len_offset: crate::heap::Heap::nursery_len_byte_offset_from_rcbox(),
        nursery_threshold: crate::nursery::Nursery::FULL_THRESHOLD,
        jit_native_result_offset: std::mem::offset_of!(ctx::ExecCtx, jit_native_result),
        globals_offset: std::mem::offset_of!(ctx::ExecCtx, globals),
        stack_data_offset,
        frame_prepushed_offset: std::mem::offset_of!(ctx::ExecCtx, jit_frame_prepushed),
        jit_resume_ip_offset: std::mem::offset_of!(ctx::ExecCtx, jit_resume_ip),
        jit_call_dest_offset: std::mem::offset_of!(ctx::ExecCtx, jit_call_dest),
        clif_call_fallback: ctx::clif_call_fallback as usize,
    }
}

impl VmClosure {
    pub fn new(
        proto: Rc<FunctionProto>,
        constants: Vec<VmValue>,
        settings: crate::settings::ExecSettings,
    ) -> Self {
        proto.ensure_ic();
        let ic_cache = Rc::clone(&proto.ic_cache);
        let feedback = Rc::clone(&proto.feedback);
        let closure = Self {
            proto,
            upvalues: Vec::new(),
            constants: Rc::new(constants),
            ic_cache,
            feedback,
        };
        // No compilation here: the compiled entry lives on the proto and is
        // produced lazily by `hot_jit_fn` once the function proves hot (see
        // `FunctionProto::jit_entry_count`). `settings.no_jit` is honoured at
        // that point, so a run meant to isolate a codegen bug never invokes
        // codegen.
        let _ = settings;
        closure
    }

    pub fn with_upvalues(
        proto: Rc<FunctionProto>,
        upvalues: Vec<VmUpvalue>,
        constants: Rc<Vec<VmValue>>,
        settings: crate::settings::ExecSettings,
    ) -> Self {
        proto.ensure_ic();
        let ic_cache = Rc::clone(&proto.ic_cache);
        let feedback = Rc::clone(&proto.feedback);
        let closure = Self {
            proto,
            upvalues,
            constants,
            ic_cache,
            feedback,
        };
        // No compilation here: the compiled entry lives on the proto and is
        // produced lazily by `hot_jit_fn` once the function proves hot (see
        // `FunctionProto::jit_entry_count`). `settings.no_jit` is honoured at
        // that point, so a run meant to isolate a codegen bug never invokes
        // codegen.
        let _ = settings;
        closure
    }

    /// Compile-time op-id resolution for `CallNativeOp` codegen: returns the
    /// native fn address, or 0 when the op-id is unknown (codegen then falls
    /// back to the runtime-resolving helper, which raises the proper error).
    fn resolve_native_op_addr(op_id: u64) -> usize {
        varn_builtins::native_op_fn(op_id).map_or(0, |f| f as usize)
    }

    fn resolve_native_op_addr_v2(op_id: u64) -> (usize, usize, varn_types::SignatureDescriptor) {
        varn_builtins::find_native_op_entry(op_id).map_or(
            (0, 0, varn_types::SignatureDescriptor::empty()),
            |e| (e.func_ptr as usize, e.raw_func_ptr as usize, e.signature)
        )
    }

    /// Frame entries a proto must accumulate before it is worth lowering.
    ///
    /// Threshold for a function WITH a back edge.
    ///
    /// This arm was pinned at 1 — compile every looping function just before
    /// its first entry — for one reason: tiering counts FRAME ENTRIES, which
    /// says nothing about a function entered once that then spins a million
    /// iterations. It reached no threshold, and there was no way back into it.
    ///
    /// [`Self::JIT_OSR_BACKEDGES`] is that way back in. A looping frame is now
    /// rescued in flight by on-stack replacement after it has PROVEN it loops,
    /// so this arm no longer has to compile on faith, and a function that
    /// loops three times and returns stops paying ~2 ms of Cranelift for it.
    ///
    /// Two earlier obstacles, both re-checked rather than inherited:
    ///
    /// * A closure/upvalue tier-parity bug used to make ANY value above 1 fail
    ///   (`import 33-globals-async-coherence` then `37-complex-closures`,
    ///   `vn bench --runs 1` → `ASSERT FAIL: sm after start`). Re-run at
    ///   `VARN_JIT_TIER` 2, 4 and 128 on 2026-08-01: both modules pass across
    ///   the warmup and timed runs. It has been fixed since; the note is kept
    ///   only so the next reader does not re-derive it from the git log.
    /// * The straight-line arm's own sweep expired once `SIZE_GATE_WORDS` grew
    ///   to 8192 — see [`Self::JIT_TIER_THRESHOLD_STRAIGHT`]. Both arms now
    ///   agree at 128, which is why they are still two constants: the value is
    ///   a measurement, and a measurement that happens to coincide is not the
    ///   same claim as one that must.
    ///
    /// Measured going from 1 to 128, paired and alternating per the protocol
    /// in `docs/superpowers/plans/2026-08-01-jit-osr.md` §6 (medians of 9
    /// alternating rounds, both binaries in one loop):
    ///
    /// ```text
    ///                      │ threshold 1 │ 128 + OSR │ ratio
    /// ─────────────────────┼─────────────┼───────────┼───────
    /// main.vn, quiet host  │   104.44 ms │  46.19 ms │ 2.26x
    /// main.vn, noisy host  │   150.98 ms │  49.39 ms │ 3.06x
    /// hot loop, 1 entry    │    51.37 ms │  49.42 ms │ noise
    /// ```
    ///
    /// Two batches, not one, because the ratio is not stable to three digits
    /// on this host: the second ran while the machine was busy and the
    /// threshold-1 column spread 112–301 ms against the OSR column's 41–67.
    /// Both batches are reported rather than the flattering one — the honest
    /// claim is "2–3x on the suite, hot loop unchanged", and the hot-loop
    /// spreads (46–76 vs 44–68) overlap almost completely.
    ///
    /// The alternation is not ceremony: within a single quiet batch the
    /// threshold-1 column still drifted from 85 ms to 104 ms while the OSR
    /// column held near 46 ms. A sequential A/B here would have reported
    /// anything from 1.8x to 3x depending only on when it ran.
    ///
    /// **This value is only sound BECAUSE OSR exists.** Same binary, same hot
    /// loop, OSR pushed out of reach with `VARN_JIT_OSR=4000000000`:
    ///
    /// ```text
    /// 128 + OSR    │ 48.8 ms · 68.8 ms · 56.8 ms
    /// 128, no OSR  │ 1.041 s · 1.430 s · 1.712 s     ← ~20x, i.e. interpreted
    /// ```
    ///
    /// A function entered once that loops never reaches 128 entries, so
    /// without the OSR arm it runs interpreted start to finish. If OSR is ever
    /// disabled or proves unsound, this constant goes back to 1 in the same
    /// change — leaving it raised is the 20x regression above.
    const JIT_TIER_THRESHOLD: u32 = 128;

    /// Threshold for a function with NO back edge. Straight-line code that has
    /// only ever been entered a couple of times is not worth ~2 ms of
    /// Cranelift: a correctness suite is mostly this shape, and compiling it
    /// all is the single biggest cost of a cold run. Code that loops takes the
    /// other arm (see `FunctionProto::has_backedge`) — it may never be entered
    /// twice, and it is [`Self::JIT_OSR_BACKEDGES`] that rescues it instead.
    ///
    /// The earlier sweep that picked 8 was run under `SIZE_GATE_WORDS = 250`,
    /// which turned every large function away before Cranelift saw it. Raising
    /// the gate to 8192 admitted exactly those functions, so the old curve no
    /// longer describes this build — a perf conclusion expires when what
    /// surrounds it changes.
    ///
    /// Re-swept on `tests/main.vn` (e2e p50, loop arm pinned at 1):
    /// 8 → 395 ms, 32 → 164, 64 → 141, 128 → 121, 512 → 129. The hot
    /// benchmarks are indifferent because their kernels all carry a back edge
    /// and take the other arm: at 128, matrix/dto/gc_alloc/json move by less
    /// than run-to-run noise.
    const JIT_TIER_THRESHOLD_STRAIGHT: u32 = 128;

    /// Back edges a proto must take before a frame still running it is
    /// compiled MID-FLIGHT and resumed in the compiled code.
    ///
    /// This is the counter frame-entry tiering cannot supply: a function
    /// entered once and then spinning never reaches any entry threshold, so
    /// without OSR the only way to catch it was to compile every looping
    /// function on its first entry — which is what
    /// [`Self::JIT_TIER_THRESHOLD`] used to be forced to do.
    ///
    /// Low, because by the time it fires the frame has already PROVEN it
    /// loops; the only cost being amortized is the lowering itself.
    ///
    /// Swept on this host (release, p50 e2e, median of 3 passes,
    /// `JIT_TIER_THRESHOLD = 128`), suite = `tests/main.vn --runs 20`,
    /// hot loop = one entry / 20M iterations / `--runs 5`:
    ///
    /// ```text
    /// VARN_JIT_OSR │ suite    │ hot loop
    /// ─────────────┼──────────┼──────────
    ///          100 │ 44.50 ms │ 43.93 ms
    ///         1000 │ 44.74 ms │ 43.73 ms
    ///        10000 │ 45.61 ms │ 43.54 ms
    ///       100000 │ 46.00 ms │ 47.55 ms
    /// ```
    ///
    /// The honest reading is that the curve is FLAT from 100 to 10000 — every
    /// difference there is inside this host's run-to-run spread — and only
    /// starts costing at 100000, where the interpreted prologue of the hot
    /// loop finally shows up. 1000 is the middle of that plateau, not a
    /// measured optimum, and picking 100 or 10000 instead would not be
    /// contradicted by this data.
    ///
    /// Like every perf constant here, the conclusion expires when what
    /// surrounds it changes: it was measured with the OSR lowering NOT
    /// mirroring registers to home slots (see `mirror_home` in
    /// `clif::lower`), which is what makes the compiled arm worth reaching.
    const JIT_OSR_BACKEDGES: u32 = 1000;

    /// The OSR back-edge threshold in force. `VARN_JIT_OSR` overrides it, so a
    /// sweep does not need one binary per value — the same arrangement
    /// `VARN_JIT_TIER` has for the entry thresholds.
    #[inline(always)]
    pub(crate) fn osr_backedge_threshold() -> u32 {
        static T: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
        *T.get_or_init(|| {
            std::env::var("VARN_JIT_OSR")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(Self::JIT_OSR_BACKEDGES)
        })
    }

    /// The threshold in force for `proto`. `VARN_JIT_TIER` overrides every arm
    /// so a sweep does not need one binary per value.
    fn tier_threshold(proto: &FunctionProto) -> u32 {
        if proto.has_backedge() {
            static LT: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
            return *LT.get_or_init(|| {
                std::env::var("VARN_JIT_TIER")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(Self::JIT_TIER_THRESHOLD)
            });
        }
        static ST: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
        *ST.get_or_init(|| {
            std::env::var("VARN_JIT_TIER_STRAIGHT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(Self::JIT_TIER_THRESHOLD_STRAIGHT)
        })
    }

    /// The compiled entry, if this proto already has one BUILT FOR THE RUNNING
    /// CONTEXT. Never compiles. The proto — not the closure — owns the entry:
    /// many closures share a proto, and tiering happens after the closures
    /// exist. The epoch test is what keeps that ownership honest across runs:
    /// the proto outlives the context, the code does not (see
    /// `FunctionProto::jit_epoch`).
    #[inline(always)]
    pub fn jit_fn(&self) -> Option<varn_jit::JitFn> {
        if self.proto.jit_epoch.get() != crate::clif_link::current_epoch()
            && !crate::clif_link::adopt_if_inherited(&self.proto)
        {
            return None;
        }
        self.proto
            .jit_entry
            .get()
            .map(|e| unsafe { std::mem::transmute::<usize, varn_jit::JitFn>(e) })
    }

    /// Count one frame entry and lower the proto once it proves hot.
    pub fn hot_jit_fn(&self) -> Option<varn_jit::JitFn> {
        if let Some(f) = self.jit_fn() {
            varn_jit::JIT_STATS
                .jit_cached
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Some(f);
        }
        if self.proto.jit_failed.get() {
            return None;
        }
        let n = self.proto.jit_entry_count.get() + 1;
        self.proto.jit_entry_count.set(n);
        if n < Self::tier_threshold(&self.proto) {
            return None;
        }
        self.compile_jit();
        self.jit_fn()
    }

    pub fn compile_jit(&self) {
        let epoch = crate::clif_link::current_epoch();
        if self.proto.jit_failed.get() || epoch == 0 {
            return;
        }
        // An entry from a PREVIOUS epoch is not a reason to skip: it is code
        // for a heap this one did not come from. Rebuild, and retire the old
        // buffer under its own epoch rather than dropping it — an outer
        // context may still be executing it (a nested run can reach a proto
        // its caller is in).
        let previous = if self.proto.jit_entry.get().is_some() {
            if self.proto.jit_epoch.get() == epoch
                || crate::clif_link::adopt_if_inherited(&self.proto)
            {
                return;
            }
            let old_epoch = self.proto.jit_epoch.get();
            self.proto
                .jit_code
                .borrow_mut()
                .take()
                .map(|code| (old_epoch, code))
        } else {
            None
        };
        let helpers = build_jit_helpers();
        let linker = crate::clif_link::CtxLinker::current();
        match varn_jit::compile(&self.proto, &self.constants, helpers, &linker, None) {
            Ok(compiled) => {
                let entry_usize: usize = unsafe { std::mem::transmute(compiled.entry) };
                self.proto.jit_epoch.set(epoch);
                self.proto
                    .jit_serial
                    .set(crate::clif_link::stamp_compile_serial());
                self.proto.jit_entry.set(Some(entry_usize));
                *self.proto.jit_code.borrow_mut() = Some(compiled.code);
                // Publish the direct entry LAST: a call site that observes a
                // non-zero `clif_raw` must find fully installed code behind it.
                self.proto.clif_raw.set(compiled.raw);
                crate::clif_link::register_compiled(&self.proto, previous);
            }
            Err(_) => {
                self.proto.jit_failed.set(true);
                self.proto.jit_entry.set(None);
                self.proto.clif_raw.set(0);
                self.proto.jit_epoch.set(0);
                if let Some((old_epoch, code)) = previous {
                    crate::clif_link::retire_code(old_epoch, code);
                }
            }
        }
    }

    /// The ON-STACK REPLACEMENT entry for `osr_ip`, lowering it on first
    /// request. `None` when this proto is not OSR-eligible or the lowering
    /// bailed — the caller then keeps interpreting.
    ///
    /// One variant per proto: the first loop to prove hot owns it, and a
    /// request for any other ip is refused rather than recompiling. A function
    /// with two hot loops is a function that will reach the ordinary entry
    /// threshold soon enough; paying a second ~2 ms lowering to catch the
    /// other one mid-flight is not obviously worth it, and nothing measured
    /// says it is.
    pub fn osr_jit_fn(&self, osr_ip: usize) -> Option<varn_jit::JitFn> {
        let proto = &self.proto;
        if proto.jit_osr_failed.get() {
            return None;
        }
        let epoch = crate::clif_link::current_epoch();
        if epoch == 0 {
            return None;
        }
        if proto.jit_osr_epoch.get() == epoch {
            if let Some(entry) = proto.jit_osr_entry.get() {
                if proto.jit_osr_ip.get() != osr_ip {
                    return None;
                }
                return Some(unsafe { std::mem::transmute::<usize, varn_jit::JitFn>(entry) });
            }
        }

        // Eligibility. An OSR entry abandons the interpreter frame in the
        // middle of its execution, so it is sound only when the frame carries
        // no interpreter-side state the compiled body does not model.
        //
        // A generator or async body suspends through the OUTERMOST clif
        // frame's jump buffer (`ExecCtx::jit_suspend_buf`); a frame resumed by
        // OSR is by construction not that frame, so `Yield`/`Await` would park
        // against a buffer belonging to someone else.
        //
        // The remaining guards are enforced where the evidence lives: the
        // caller checks for an active `try` handler in this frame (only the
        // `ExecCtx` knows), and the lowering itself refuses an `osr_ip` that
        // is not a block start, a body containing `CallSelf`, and anything
        // over the size gate.
        if proto.is_generator || proto.is_async {
            proto.jit_osr_failed.set(true);
            return None;
        }

        let helpers = build_jit_helpers();
        let linker = crate::clif_link::CtxLinker::current();
        match varn_jit::compile(proto, &self.constants, helpers, &linker, Some(osr_ip)) {
            Ok(compiled) => {
                let entry_usize: usize = unsafe { std::mem::transmute(compiled.entry) };
                proto.jit_osr_epoch.set(epoch);
                proto.jit_osr_ip.set(osr_ip);
                proto.jit_osr_entry.set(Some(entry_usize));
                *proto.jit_osr_code.borrow_mut() = Some(compiled.code);
                // `compiled.raw` is 0 for every OSR lowering (they are forced
                // frame-aware), so there is deliberately nothing to publish in
                // `clif_raw`: a resume prologue must never become a call
                // target.
                debug_assert_eq!(compiled.raw, 0, "osr lowering must not publish a raw entry");
                // Registered so the epoch holds the proto — and therefore the
                // buffer — alive for as long as code from it can run.
                crate::clif_link::register_compiled(proto, None);
                Some(unsafe { std::mem::transmute::<usize, varn_jit::JitFn>(entry_usize) })
            }
            Err(_) => {
                proto.jit_osr_failed.set(true);
                proto.jit_osr_entry.set(None);
                proto.jit_osr_epoch.set(0);
                None
            }
        }
    }

    #[inline(always)]
    pub fn ic_cache_len(&self) -> usize {
        self.ic_cache.borrow().len()
    }
}

#[repr(C)]
pub struct CallFrame {
    pub closure_ptr: *const VmClosure,
    pub _owned_closure: Option<Rc<VmClosure>>,
    pub ip: usize,
    pub base: usize,
    pub current_class: Option<Rc<varn_types::ClassObj>>,
    pub return_reg: Option<u16>,
    pub caller_base: Option<usize>,
}

unsafe impl Send for CallFrame {}
unsafe impl Sync for CallFrame {}

impl CallFrame {
    pub fn new(closure: &VmClosure, base: usize) -> Self {
        Self {
            closure_ptr: closure as *const VmClosure,
            _owned_closure: None,
            ip: 0,
            base,
            current_class: None,
            return_reg: None,
            caller_base: None,
        }
    }

    pub fn new_owned(closure: Rc<VmClosure>, base: usize) -> Self {
        Self {
            closure_ptr: Rc::as_ptr(&closure),
            _owned_closure: Some(closure),
            ip: 0,
            base,
            current_class: None,
            return_reg: None,
            caller_base: None,
        }
    }

    #[inline(always)]
    pub fn closure(&self) -> &VmClosure {
        unsafe { &*self.closure_ptr }
    }

    #[inline(always)]
    pub fn proto(&self) -> &FunctionProto {
        &self.closure().proto
    }

    #[inline(always)]
    pub fn code(&self) -> &[u16] {
        &self.closure().proto.chunk.code
    }

    #[inline(always)]
    pub fn read_u16(&self) -> u16 {
        self.code()[self.ip]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TryHandler {
    pub catch_ip: usize,
    pub frame_depth: usize,
    pub err_reg: u8,
}

#[derive(Debug, Clone)]
pub struct VmClosurePayload(pub Rc<VmClosure>);

impl VmValuePayload for VmClosurePayload {
    fn clone_payload(&self) -> Box<dyn VmValuePayload> {
        Box::new(self.clone())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod jit_epoch_tests {
    use crate::clif_link::{current_epoch, invalidate_epoch, CtxGuard};
    use crate::exec::ExecCtx;
    use crate::globals::GlobalStore;
    use crate::settings::ExecSettings;

    /// The smallest proto that can carry a compiled entry. Only the JIT cells
    /// matter here; nothing executes it.
    fn bare_proto() -> varn_types::FunctionProto {
        use varn_types::register_meta::SlotKind;
        varn_types::FunctionProto {
            name: Some("victim".into()),
            arity: 1,
            export_names: Vec::new(),
            register_count: 1,
            has_rest: false,
            is_async: false,
            is_generator: false,
            has_this: false,
            upvalue_count: 0,
            cache_count: 0,
            chunk: varn_types::chunk::Chunk::new(),
            required_caps: Vec::new(),
            register_meta: Vec::new(),
            param_kinds: Vec::new(),
            return_kind: SlotKind::Dynamic,
            resolved_shapes: std::cell::RefCell::new(Vec::new()),
            jit_entry: std::cell::Cell::new(None),
            clif_raw: std::cell::Cell::new(0),
            jit_code: std::cell::RefCell::new(None),
            jit_failed: std::cell::Cell::new(false),
            jit_epoch: std::cell::Cell::new(0),
            jit_serial: std::cell::Cell::new(0),
            backedge_memo: std::cell::Cell::new(0),
            ic_cache: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
            feedback: std::rc::Rc::new(std::cell::RefCell::new(
                varn_types::chunk::FeedbackVector::new(0),
            )),
            static_closure_val: std::cell::Cell::new(0),
            jit_entry_count: std::cell::Cell::new(0),
            backedge_count: std::cell::Cell::new(0),
            jit_osr_entry: std::cell::Cell::new(None),
            jit_osr_epoch: std::cell::Cell::new(0),
            jit_osr_ip: std::cell::Cell::new(0),
            jit_osr_code: std::cell::RefCell::new(None),
            jit_osr_failed: std::cell::Cell::new(false),
        }
    }

    fn ctx() -> ExecCtx {
        ExecCtx::new(
            GlobalStore::new(),
            ExecSettings {
                no_jit: true,
                trace: false,
            },
        )
    }

    /// Compiled code bakes handles into one heap's object table, so two heaps
    /// must never be told they can share it. This is the invariant behind the
    /// `"a" + <object> + "b"` miscompile: a proto outlives the context that
    /// compiled it, and the next run got the old entry.
    #[test]
    fn each_heap_is_its_own_epoch() {
        let a = ctx();
        let b = ctx();
        assert_ne!(a.heap.jit_epoch(), b.heap.jit_epoch());

        // A nested context over the SAME objects keeps the epoch: it reaches
        // the very handles the code baked, so that code stays valid.
        let shared = a.heap.clone();
        assert_eq!(shared.jit_epoch(), a.heap.jit_epoch());

        // A deep copy does not: same indices, different objects.
        assert_ne!(a.heap.deep_clone().jit_epoch(), a.heap.jit_epoch());
    }

    #[test]
    fn the_guard_publishes_the_running_heap() {
        let c = ctx();
        assert_eq!(current_epoch(), 0, "no epoch outside a run");
        {
            let _g = CtxGuard::enter(&c as *const ExecCtx);
            assert_eq!(current_epoch(), c.heap.jit_epoch());
        }
        assert_eq!(current_epoch(), 0, "the guard restores what it found");
    }

    /// Ending an epoch must strip the entries compiled under it — the protos
    /// holding them are still alive and reachable by the next run, and a stale
    /// `clif_raw` is jumped to directly by other compiled code.
    #[test]
    fn invalidating_an_epoch_strips_its_entries() {
        let c = ctx();
        let epoch = c.heap.jit_epoch();
        let proto = std::rc::Rc::new(bare_proto());
        proto.jit_entry.set(Some(0xdead_beef));
        proto.clif_raw.set(0xdead_beef);
        proto.jit_epoch.set(epoch);
        {
            let _g = CtxGuard::enter(&c as *const ExecCtx);
            crate::clif_link::register_compiled(&proto, None);
        }
        invalidate_epoch(epoch);
        assert_eq!(proto.jit_entry.get(), None);
        assert_eq!(proto.clif_raw.get(), 0);
    }
}

#[cfg(test)]
mod build_jit_helpers_tests {
    #[test]
    fn build_jit_helpers_has_real_addresses() {
        let h = super::build_jit_helpers();
        // Function-address fields must be non-zero (real code), and the
        // probed nursery threshold must be the real one, proving this is the
        // production construction and not a zeroed stub.
        assert_ne!(h.add, 0);
        assert_ne!(h.gc_safepoint, 0);
        assert_ne!(h.clif_call_fallback, 0);
        assert!(h.nursery_threshold > 0);
    }
}
