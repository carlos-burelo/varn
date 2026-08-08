pub mod clif;
mod helper_abi;
pub(crate) mod loop_hoist;
pub mod mem;
pub mod stats;

pub use stats::{CompileOutcome, CompileRecord, JitStats, JitStatsSnapshot, JIT_STATS};

/// Loop-invariant array-guard hoisting diagnostics — see
/// `loop_hoist::diagnose_loops`'s docs. Exposed for `vn debug -p bytecode`
/// (via `varn-debug`); the rest of `loop_hoist` (the actual codegen-facing
/// `plan_hoists`/`HoistPlan`) stays crate-private.
pub use loop_hoist::{
    diagnose_loops, is_alloc_free_op, CacheSource, HoistCandidate, LoopDiagnostic,
};

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

/// Largest number of bytes of `Option<HeapObj>` the JIT's string template can
/// hold. `template` below is captured at this fixed size regardless of the
/// probed `slot_size`, so the buffer only needs widening if `HeapObj` grows
/// past it — the probe asserts that at startup instead of silently
/// truncating.
pub const STR_TEMPLATE_MAX: usize = 64;

/// Probed layout facts for the JIT's inline string allocation.
///
/// Stage B writes a `HeapObj::Str(HeapStr::Inline { .. })` straight into a
/// nursery slot from generated code. `Option<HeapObj>`'s encoding and
/// `HeapStr::Inline`'s field offsets inside it are not guaranteed by Rust, so
/// nothing here is hardcoded: `template` is a real value captured as bytes,
/// and every other field is measured against that same value (see
/// `Heap::jit_str_layout`, which follows `JitArrayLayout`'s and
/// `JitObjectLayout`'s precedent of probing rather than assuming).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitStrLayout {
    /// Discriminant byte value of `HeapObj::Str` (niche-shared with `Option`).
    pub str_tag: usize,
    /// A ready-made `Some(HeapObj::Str(HeapStr::Inline { len: 0, ascii:
    /// UNKNOWN, bytes: [0; INLINE_STR_CAP] }))`, captured as raw bytes.
    /// Emitted code stores `slot_size` bytes of this and then overwrites
    /// `len_off` and the payload — so it never has to understand the
    /// discriminant or the `ascii` cell.
    pub template: [u8; STR_TEMPLATE_MAX],
    /// `size_of::<Option<HeapObj>>()` — how much of `template` is live.
    pub slot_size: usize,
    /// Slot base → the `Inline` variant's `len: u8`.
    pub len_off: usize,
    /// Slot base → the `Inline` variant's `bytes[0]`.
    pub bytes_off: usize,
    /// `INLINE_STR_CAP` — the largest result the inline arm may build.
    pub inline_cap: usize,
    /// RcBox base → the nursery `forwarding` Vec's three words.
    pub nursery_fwd_vec_off: usize,
    /// RcBox base → `Nursery::alloc_count`.
    pub alloc_count_off: usize,
    /// `NURSERY_CAPACITY` — the bound the emitted bump checks against.
    pub nursery_capacity: usize,
    /// Raw bytes of `Option::<u32>::None`, captured by the probe. Written
    /// into a freshly bumped `forwarding` slot so it never reads back as a
    /// stale `Some` left over by `Nursery::collect` (which clears length
    /// without zeroing the backing bytes).
    pub fwd_none_pattern: u64,
    /// `size_of::<Option<u32>>()`. Asserted `== 8` at the probe: emitted code
    /// always stores `fwd_none_pattern` as a single 8-byte write, which is
    /// only correct at that width.
    pub fwd_elem_size: usize,
}

// `derive(Default)` cannot cover `[u8; STR_TEMPLATE_MAX]`: std only
// implements `Default` for arrays up to length 32, and `STR_TEMPLATE_MAX` is
// 64. Hand-written for the same all-zero result the derive would have given
// every other (all-`usize`) field.
impl Default for JitStrLayout {
    fn default() -> Self {
        JitStrLayout {
            str_tag: 0,
            template: [0u8; STR_TEMPLATE_MAX],
            slot_size: 0,
            len_off: 0,
            bytes_off: 0,
            inline_cap: 0,
            nursery_fwd_vec_off: 0,
            alloc_count_off: 0,
            nursery_capacity: 0,
            fwd_none_pattern: 0,
            fwd_elem_size: 0,
        }
    }
}

/// Generates [`JitHelpers`] from the one shared list in
/// [`crate::jit_helper_abi`]. Every entry there becomes a `usize` holding a
/// host function address; the tail below is hand-written because those
/// fields are not function addresses.
macro_rules! define_jit_helpers {
    ( $( $(#[$attr:meta])* $field:ident => $_vm_fn:ident ),* $(,)? ) => {
        #[derive(Debug, Clone, Copy)]
        #[repr(C)]
        pub struct JitHelpers {
            $( $(#[$attr])* pub $field: usize, )*
        /// Compile-time op-id → native call target. See
        /// [`varn_types::NativeOpTarget`] for what each field means and what a
        /// zero in it implies.
        ///
        /// Resolved at LOWERING time and embedded in the generated code: the
        /// op-id form pays a hash lookup on every runtime call, which on a hot
        /// `arr.push(x)` is a large share of the call's whole cost.
        ///
        /// A function pointer rather than a direct call because `varn-jit` does
        /// not depend on `varn-builtins` — this is the indirection that keeps
        /// the op table on the VM side of the boundary.
        pub resolve_native_op: fn(u64) -> varn_types::NativeOpTarget,
        /// Probed heap/array layout for the inline array-read fast path.
        pub array_layout: JitArrayLayout,
        /// Probed object layout for the inline property get/set fast paths.
        pub object_layout: JitObjectLayout,
        /// Probed string-slot layout for the inline concat allocation path.
        pub str_layout: JitStrLayout,
        pub open_upvalues_offset: usize,
        pub pending_constructors_offset: usize,
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
        }
    };
}

crate::jit_helper_abi!(define_jit_helpers);

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
