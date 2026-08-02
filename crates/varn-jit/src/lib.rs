pub mod clif;
pub(crate) mod loop_hoist;
pub mod mem;
pub mod stats;

pub use stats::{CompileOutcome, CompileRecord, JitStats, JitStatsSnapshot, JIT_STATS};

/// Loop-invariant array-guard hoisting diagnostics — see
/// `loop_hoist::diagnose_loops`'s docs. Exposed for `vn debug -p bytecode`
/// (via `varn-debug`); the rest of `loop_hoist` (the actual codegen-facing
/// `plan_hoists`/`HoistPlan`) stays crate-private.
pub use loop_hoist::{diagnose_loops, is_alloc_free_op, CacheSource, HoistCandidate, LoopDiagnostic};

/// Re-exported so `varn-debug` can name the host ISA type (from
/// `clif::shared_isa()`) without taking a direct `cranelift-codegen` dep.
pub use cranelift_codegen::isa::OwnedTargetIsa;

use std::any::Any;
use std::rc::Rc;
use varn_types::FunctionProto;
use varn_types::VmValue;

pub type JitFn = unsafe extern "C" fn(
    ctx: *mut std::ffi::c_void,
    closure: *const std::ffi::c_void,
    base: usize,
    exec_ctx: *mut std::ffi::c_void,
) -> VmValue;

/// Byte offsets and probed layout facts that let emitted code walk from a
/// heap-tagged `VmValue` to an array element without any FFI call:
///
/// `[ExecCtx + heap_field] → RcBox → HeapInner.objects (Vec words) → slot
/// (Option<HeapObj>, tag byte + payload Rc) → RcBox → Vec<VmValue> words →
/// data[idx]`.
///
/// Rust does not guarantee `Vec`'s field order, so the ptr/len word offsets
/// are PROBED at startup against vectors with known contents — stable for
/// the lifetime of one binary, which is exactly the lifetime of any JIT
/// code that embeds them.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitArrayLayout {
    /// RcBox base → the old-gen `objects` Vec's three words inside HeapInner.
    pub slots_vec_off: usize,
    /// RcBox base → the nursery's `objects` Vec's three words. Heap indices
    /// use bit 31 to distinguish old gen (set) from nursery (clear).
    pub nursery_slots_vec_off: usize,
    /// Word offset of the data pointer inside `Vec<Option<HeapObj>>`.
    pub slots_ptr_off: usize,
    /// `size_of::<Option<HeapObj>>()` — slot stride.
    pub slot_size: usize,
    /// Discriminant byte value of `HeapObj::Array` (niche-shared by the
    /// `Option` wrapper).
    pub array_tag: usize,
    /// Slot base → the array payload's Rc pointer.
    pub payload_off: usize,
    /// Byte offset, from the `ArrayRepr` base (i.e. from payload RcBox + 16),
    /// of the `#[repr(C, u8)]` discriminant. `0` in practice; the inline fast
    /// paths load this byte and take the generic helper unless it is
    /// `ArrayRepr::Boxed` (0). Before Task A.4 only Boxed arrays exist, so the
    /// guard never fires — but it keeps the raw-`Vec` loads below sound the
    /// moment typed reprs appear.
    pub disc_off: usize,
    /// Byte offsets of (data ptr, len) of the element `Vec`, measured **from
    /// the `ArrayRepr` base** (payload RcBox + 16). They already include the
    /// discriminant tag + alignment padding, so `payload + 16 + off` lands
    /// directly on the `Vec`'s words for the `Boxed` variant.
    pub elems_ptr_off: usize,
    pub elems_len_off: usize,
}

/// Probed layout facts for the JIT's inline property fast paths.
///
/// `[slot + object_payload_off] → ObjData` — the object's fields live in the
/// same allocation as its header (a DST tail), so the field buffer is reached
/// with a constant `lea` off the data pointer instead of loading a separate
/// `Vec` pointer.
///
/// Every offset here is PROBED against a real object at startup rather than
/// hardcoded: the previous fast paths baked in `Vec`'s field order as the magic
/// constants 32/40/48, which is exactly the kind of assumption that turns a
/// representation change into a silent segfault.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct JitObjectLayout {
    /// Discriminant byte value of `HeapObj::Object`.
    pub object_tag: usize,
    /// Slot base → the object's `Rc<ObjData>` data pointer. `ObjRef` is a fat
    /// pointer, so the slot also carries a length word; the fast paths read the
    /// tail length from the header instead and ignore it.
    pub payload_off: usize,
    /// Data pointer → `ObjData.inline_len` (u32): how many fields live in the
    /// tail. Fields past it spilled to the overflow store, which the JIT does
    /// not know how to read — the bounds check against this value is what sends
    /// those slots to the interpreter helper.
    pub len_off: usize,
    /// Data pointer → `ObjData.values[0]`. Constant, because the tail is inline.
    pub values_off: usize,
    /// Data pointer → `ObjData.shape` (an `Rc<Shape>`).
    pub shape_off: usize,
    /// Shape pointer → `Shape.id` (u32).
    pub shape_id_off: usize,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitHelpers {
    pub load_const: usize,
    pub load_global_idx: usize,
    pub store_global_idx: usize,
    pub define_global_idx: usize,
    pub eq: usize,
    pub neq: usize,
    pub lt: usize,
    pub lte: usize,
    pub gt: usize,
    pub gte: usize,
    pub add: usize,
    pub sub: usize,
    pub mul: usize,
    pub div: usize,
    pub modulo: usize,
    pub pow: usize,
    pub to_string: usize,
    pub load_global: usize,
    pub load_upvalue: usize,
    pub store_upvalue: usize,
    pub make_closure: usize,
    pub load_static_fn: usize,
    pub call: usize,
    pub call_method: usize,
    pub call_method_flat: usize,
    pub invoke_virtual_flat: usize,
    pub get_property: usize,
    /// Flat-args variant of `get_property` for the CLIF backend:
    /// `fn(ctx, closure, obj, name_idx, cs_idx, dest, ip) -> VmValue`.
    pub get_property_flat: usize,
    pub set_property: usize,
    /// Flat-args variant of `set_property` for the CLIF backend:
    /// `fn(ctx, closure, obj, val, name_idx, cs_idx, ip)`.
    pub set_property_flat: usize,
    pub build_array: usize,
    pub build_str: usize,
    pub negate: usize,
    pub logical_not: usize,
    pub get_index: usize,
    pub set_index: usize,
    pub jit_array_get_fast: usize,
    pub jit_array_set_fast: usize,
    pub typeof_val: usize,
    pub instanceof: usize,
    pub array_length: usize,
    pub array_push: usize,
    pub array_pop: usize,
    pub array_extend: usize,
    pub str_concat: usize,
    pub str_slice: usize,
    pub str_length: usize,
    pub bit_and: usize,
    pub bit_or: usize,
    pub bit_xor: usize,
    pub shl: usize,
    pub shr: usize,
    pub ushr: usize,
    pub load_module: usize,
    pub load_module_slot: usize,
    pub store_module_slot: usize,
    pub build_object_with_shape: usize,
    pub range: usize,
    pub assert_not_null: usize,
    pub close_upvalue: usize,
    pub get_enum_tag: usize,
    pub is_array: usize,
    pub wrap_spread: usize,
    pub object_keys: usize,
    pub op_in: usize,
    pub object_merge: usize,
    pub get_fixed_field: usize,
    pub set_fixed_field: usize,
    pub get_property_maybe: usize,
    pub get_super: usize,
    pub get_symbol: usize,
    pub bind_method: usize,
    pub define_global: usize,
    pub store_global: usize,
    pub declare_field: usize,
    pub make_class: usize,
    pub inherit: usize,
    pub class_member_op: usize,
    pub build_object: usize,
    pub object_rest: usize,
    pub make_enum_variant: usize,
    pub spawn: usize,
    pub call_spread: usize,
    pub load_module_by_idx: usize,
    pub invoke_virtual: usize,
    pub try_push: usize,
    pub try_pop: usize,
    pub throw: usize,
    pub await_helper: usize,
    pub yield_helper: usize,
    pub get_property_ic_fast: usize,
    pub get_property_maybe_ic_fast: usize,
    pub jit_prepare_call: usize,
    pub jit_push_self_frame: usize,
    pub jit_post_call: usize,
    pub jit_ensure_stack_capacity: usize,
    pub dispatch_intrinsic: usize,
    pub jit_is_native_fn: usize,
    pub jit_call_native_fast: usize,
    pub jit_call_native_op: usize,
    /// `extern "C" fn(*mut ExecCtx, fn_addr, args_start, total)` — direct
    /// native call with the function pointer already resolved.
    pub jit_call_native_fnptr: usize,
    /// Compile-time op-id → native fn address resolution (0 = unknown).
    /// Lets `CallNativeOp` embed the target directly instead of paying a
    /// hash lookup on every runtime call.
    pub resolve_native_op: fn(u64) -> usize,
    pub resolve_native_op_v2: fn(u64) -> (usize, usize, varn_types::SignatureDescriptor),
    /// Probed heap/array layout for the inline array-read fast path.
    pub array_layout: JitArrayLayout,
    /// Probed object layout for the inline property get/set fast paths.
    pub object_layout: JitObjectLayout,
    pub open_upvalues_offset: usize,
    pub pending_constructors_offset: usize,
    /// `extern "C" fn(*mut ExecCtx)` — loop back-edge GC safepoint.
    pub gc_safepoint: usize,
    /// Byte offset of the heap field (an Rc, i.e. one pointer) inside ExecCtx.
    pub heap_field_offset: usize,
    /// Byte offset from the heap RcBox pointer to the nursery live-object count.
    pub nursery_len_offset: usize,
    /// Nursery fill level at which the safepoint must run.
    pub nursery_threshold: usize,
    pub jit_native_result_offset: usize,
    pub globals_offset: usize,
    /// Byte offset within `ExecCtx` of the `stack` `Vec<VmValue>`'s data
    /// pointer word (`offset_of!(ExecCtx, stack) + slots_ptr_off`, the bare-Vec
    /// ptr offset — NOT `elems_ptr_off`, which is `ArrayRepr`-relative). The
    /// allocating clif path loads this fresh each time it addresses a
    /// register's `ctx.stack` home slot, so a stack reallocation can never
    /// leave a stale base.
    pub stack_data_offset: usize,
    /// Byte offset of ExecCtx.jit_frame_prepushed — the caller→prologue
    /// frame handshake word (see its doc in varn-vm).
    pub frame_prepushed_offset: usize,
    /// Byte offset of ExecCtx.jit_resume_ip — the caller's post-call resume
    /// ip, written before a fast JIT→JIT call so an exception unwinding
    /// through this caller can resume it interpreted (see its doc in varn-vm).
    pub jit_resume_ip_offset: usize,
    /// Byte offset of ExecCtx.jit_call_dest — the caller dest register stamped
    /// as the callee frame's return_reg for correct interpreted-resume returns.
    pub jit_call_dest_offset: usize,
    /// `extern "C" fn(*mut ExecCtx, callee: VmValue, argc, a0..a3) -> VmValue`
    /// — the CLIF static-call IC miss path: dispatch the (rebound or
    /// GC-moved) callee through the interpreter/JIT with boxed args.
    pub clif_call_fallback: usize,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitGetIndexArgs {
    pub obj: VmValue,
    pub key: VmValue,
    pub dest: usize,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitSetIndexArgs {
    pub obj: VmValue,
    pub key: VmValue,
    pub val: VmValue,
}

use std::sync::atomic::Ordering;

use stats::record;

/// Bytecode length, in words, above which a function is refused before
/// Cranelift is asked. See the gate in [`compile`] for why it exists.
pub const SIZE_GATE_WORDS: usize = 8192;

fn fn_name(proto: &FunctionProto) -> String {
    proto.name.as_deref().unwrap_or("<module>").to_owned()
}

/// What a successful compilation hands back to the VM.
///
/// `raw` is the direct clif→clif entry, or `0` when the function took the
/// frame-aware lowering — such a raw expects `(stack_ptr, closure, base, …)`,
/// a callee frame no caller can supply, so only the wrapper may invoke it.
/// The VM publishes `raw` in `FunctionProto::clif_raw` for other functions'
/// call sites to load.
pub struct Compiled {
    pub entry: JitFn,
    pub raw: usize,
    pub code: Rc<dyn Any>,
}

/// Lower `proto` for the running context.
///
/// `osr_ip` picks the entry shape. `None` is the ordinary one. `Some(ip)`
/// builds an ON-STACK REPLACEMENT entry: same body, but a parameterless
/// prologue that reloads the register file from the live frame and resumes at
/// `ip`, so a function that was entered once and is still looping can be
/// compiled without waiting for a second entry that may never come. Such a
/// lowering is always frame-aware, so [`Compiled::raw`] comes back `0` and no
/// call site can reach the resume prologue.
pub fn compile(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: JitHelpers,
    linker: &dyn clif::lower::ClifLinker,
    osr_ip: Option<usize>,
) -> Result<Compiled, String> {
    // NOT a compile-time budget: this cap is what keeps module top-levels
    // (and other long functions) out of clif, and with them the only shapes
    // that put one clif frame underneath another.
    //
    // A clif frame is not resumable — it has no bytecode `ip` of its own — and
    // `execute_jit_frame` installs a single `setjmp` for the OUTERMOST clif
    // frame only. So a `throw` inside a nested clif call unwinds the native
    // stack of every clif frame in between while their `CallFrame`s stay live
    // with `ip == 0`, and the frame loop re-enters them from the top: an
    // endless re-execution (`assert(safeDivide(10, 0) === -1)` in
    // tests/11-errors.vn hangs allocating). Suspension (`Await`, `Yield`) has
    // the same hole, minus the loop.
    //
    // Lifting this cap therefore means making clif frames resumable (a
    // per-frame jump buffer for exceptions plus a side-exit ip for
    // suspension), not just raising the number.
    let words = proto.chunk.code.len();
    if words > SIZE_GATE_WORDS {
        // Traced like a lowering bail: this gate fires BEFORE clif is asked, so
        // a function rejected here shows up in neither `CLIF BAIL` nor
        // `compile_fail`. Counting "0 bails" without it overstates coverage.
        if clif::trace() {
            eprintln!("CLIF GATE  {:?}: too large ({words} words)", proto.name);
        }
        JIT_STATS.gate_rejected.fetch_add(1, Ordering::Relaxed);
        record(|| CompileRecord {
            name: fn_name(proto),
            words,
            outcome: CompileOutcome::Gated("too large (>250 words)"),
            compile_ns: 0,
            code_bytes: 0,
        });
        return Err("JIT Bailout: function too large".to_owned());
    }

    // Everything routes through the Cranelift backend; a bail leaves the
    // function to the interpreter.
    if clif::enabled() {
        if let Ok(isa) = clif::shared_isa() {
            let start = std::time::Instant::now();
            let res =
                clif::lower::try_compile(proto, constants, &helpers, isa, linker, osr_ip, None);
            let elapsed = start.elapsed().as_nanos() as u64;
            match res {
                Ok(art) => {
                    if clif::trace() {
                        eprintln!("CLIF ROUTE {:?}", proto.name);
                    }
                    let code_bytes = art.buffer.size() as u64;
                    JIT_STATS.compile_success.fetch_add(1, Ordering::Relaxed);
                    JIT_STATS
                        .total_compile_time_ns
                        .fetch_add(elapsed, Ordering::Relaxed);
                    JIT_STATS
                        .total_code_size_bytes
                        .fetch_add(code_bytes, Ordering::Relaxed);
                    record(|| CompileRecord {
                        name: fn_name(proto),
                        words,
                        outcome: CompileOutcome::Routed,
                        compile_ns: elapsed,
                        code_bytes,
                    });
                    let jit_fn: JitFn = unsafe { std::mem::transmute(art.entry) };
                    let raw = if art.frame_aware { 0 } else { art.raw as usize };
                    return Ok(Compiled {
                        entry: jit_fn,
                        raw,
                        code: Rc::new(art) as Rc<dyn Any>,
                    });
                }
                Err(e) => {
                    if clif::trace() {
                        eprintln!("CLIF BAIL  {:?}: {e}", proto.name);
                    }
                    JIT_STATS.compile_fail.fetch_add(1, Ordering::Relaxed);
                    JIT_STATS
                        .total_compile_time_ns
                        .fetch_add(elapsed, Ordering::Relaxed);
                    record(|| CompileRecord {
                        name: fn_name(proto),
                        words,
                        outcome: CompileOutcome::Bailed(e.clone()),
                        compile_ns: elapsed,
                        code_bytes: 0,
                    });
                    return Err(e);
                }
            }
        }
    }

    Err("JIT disabled or unsupported proto".into())
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitCallArgs {
    pub callee: VmValue,
    pub arg_start: usize,
    pub arg_count: usize,
    pub dest: usize,
    pub ip: usize,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitCallMethodArgs {
    pub this_val: VmValue,
    pub name_idx: usize,
    pub cs: usize,
    pub arg_start: usize,
    pub arg_count: usize,
    pub dest: usize,
    pub ip: usize,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitGetPropertyArgs {
    pub obj: VmValue,
    pub name_idx: usize,
    pub cs_idx: usize,
    pub dest: usize,
    pub ip: usize,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitSetPropertyArgs {
    pub obj: VmValue,
    pub val: VmValue,
    pub name_idx: usize,
    pub cs_idx: usize,
    pub ip: usize,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitInvokeVirtualArgs {
    pub this_val: VmValue,
    pub name_idx: usize,
    pub arg_start: usize,
    pub arg_count: usize,
    pub dest: usize,
    pub ip: usize,
}
