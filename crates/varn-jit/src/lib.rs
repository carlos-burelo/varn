pub mod aot;
pub mod clif;
pub(crate) mod loop_hoist;
pub mod mem;
pub mod stack_roots;
pub mod stats;

pub use stats::{CompileOutcome, CompileRecord, JitStats, JitStatsSnapshot, JIT_STATS};

/// Loop-invariant array-guard hoisting diagnostics — see
/// `loop_hoist::diagnose_loops`'s docs. Exposed for `vn debug -p bytecode`
/// (via `varn-debug`); the rest of `loop_hoist` (the actual codegen-facing
/// `plan_hoists`/`HoistPlan`) stays crate-private.
pub use loop_hoist::{
    diagnose_loops, is_alloc_free_op, CacheSource, HoistCandidate, LoopDiagnostic,
};

/// Contrato ABI v2: única fuente del layout caliente (spec §1-§2).
/// `varn-jit` y `varn-vm` lo nombran desde aquí; ninguno lo redefine.
pub use varn_abi;

/// Re-exported so `varn-debug` can name the host ISA type (from
/// `clif::shared_isa()`) without taking a direct `cranelift-codegen` dep.
pub use cranelift_codegen::isa::OwnedTargetIsa;

use std::any::Any;
use std::rc::Rc;
use varn_op_macros::jit_helper_table;
use varn_types::FunctionProto;
use varn_types::VmValue;

pub type JitFn = unsafe extern "C" fn(
    ctx: *mut std::ffi::c_void,
    closure: *const std::ffi::c_void,
    base: usize,
    exec_ctx: *mut std::ffi::c_void,
) -> VmValue;

/// Byte offsets and measured layout facts that let emitted code walk from a
/// heap-tagged `VmValue` to an array element without any FFI call:
///
/// `[ExecCtx + heap_field] → RcBox → HeapInner.objects (Vec words) → slot
/// (Option<HeapObj>, tag byte + payload Rc) → RcBox → Vec<VmValue> words →
/// data[idx]`.
///
/// Two provenances, kept apart on purpose: offsets into OUR `repr(C)` types
/// (`ArrayRepr::{DISC_OFF, ELEMS_PTR_OFF, ELEMS_LEN_OFF}`, `size_of`) are
/// derived exacto (Ley 6); offsets through FOREIGN memory (`Vec` word order,
/// `Option` niche, `RcBox` prefix) are measured once at startup — stable for
/// the lifetime of one binary, which is exactly the lifetime of any JIT
/// code that embeds them.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitArrayLayout {
    /// Object address → its header's generation-state byte.
    pub state_off: usize,
    /// Object address → its header's kind byte: the `HeapObj` discriminant
    /// for objects stored as one.
    pub kind_off: usize,
    /// The state byte of a young object.
    pub young_state: usize,
    /// Word offset of the data pointer inside a `Vec`.
    pub vec_ptr_off: usize,
    /// Discriminant byte value of `HeapObj::Array` (niche-shared by the
    /// `Option` wrapper).
    pub array_tag: usize,
    /// Discriminant byte value of `HeapObj::Str`.
    pub str_tag: usize,
    /// Slot base → the array payload's Rc pointer.
    pub payload_off: usize,
    /// Byte offset, from the `ArrayRepr` base (the address the payload holds),
    /// of the `#[repr(C, u8)]` discriminant. `0` in practice; the inline fast
    /// paths load this byte and take the generic helper unless it is
    /// `ArrayRepr::Boxed` (0). `ArrayRepr` now also has `I64`/`F64` and 7
    /// narrow-int/float variants (discriminants 1..9); the guard treats all
    /// of them alike (anything non-zero takes the generic helper) and keeps
    /// the raw-`Vec` loads below sound regardless of which typed repr shows
    /// up at this offset.
    pub disc_off: usize,
    /// Byte offsets of (data ptr, len) of the element `Vec`, measured **from
    /// the `ArrayRepr` base** (the address the payload holds). They already
    /// include the discriminant tag + alignment padding, so `payload + off` lands
    /// directly on the `Vec`'s words for the `Boxed` variant.
    pub elems_ptr_off: usize,
    pub elems_len_off: usize,
}

/// Layout facts for the JIT's inline property fast paths.
///
/// `[slot + object_payload_off] → ObjData` — the object's fields live in the
/// same allocation as its header (a DST tail), so the field buffer is reached
/// with a constant `lea` off the data pointer instead of loading a separate
/// `Vec` pointer.
///
/// Offsets into OUR `repr(C)` types (`OBJ_*`, `INST_*`, `SHAPE_ID_OFF`) are
/// derived exacto; the `Option` niche tag/payload and the `RcBox` prefix stay
/// measured (foreign). The old word-scans over owned structs are gone —
/// derivation plus tripwires, not searches.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct JitObjectLayout {
    /// Discriminant byte value of `HeapObj::Object`.
    pub object_tag: usize,
    /// Discriminant byte value of `HeapObj::Instance`.
    pub instance_tag: usize,
    /// Slot base → the object's `Rc<ObjData>` data pointer. `ObjRef` is a fat
    /// pointer, so the slot also carries a length word; the fast paths read the
    /// tail length from the header instead and ignore it.
    pub payload_off: usize,
    /// Slot base → the instance's `Rc<InstanceData>` pointer. `InstanceRef` is
    /// a thin pointer (Rc<InstanceData>), so offset in the slot matches payload_off.
    pub instance_payload_off: usize,
    /// Data pointer → `ObjData.inline_len` (u32): how many fields live in the
    /// tail. Fields past it spilled to the overflow store, which the JIT does
    /// not know how to read — the bounds check against this value is what sends
    /// those slots to the interpreter helper.
    pub len_off: usize,
    /// Data pointer → `ObjData.values[0]`. Constant, because the tail is inline.
    pub values_off: usize,
    /// Data pointer → `InstanceData.payload`.
    pub instance_values_off: usize,
    /// Data pointer → `InstanceData.class_id` (u32). The 8-byte header sits
    /// right before the payload.
    pub instance_class_id_off: usize,
    /// Data pointer → `ObjData.shape` (an `Rc<Shape>`).
    pub shape_off: usize,
    /// Shape pointer → `Shape.id` (u32).
    pub shape_id_off: usize,
}

/// What a compiled call site needs to enter a native activation without the
/// runtime: walk a callee value to its closure and proto, read the published
/// native entry, and push and pop the `CallFrame` by hand.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct JitCallLayout {
    /// Discriminant byte of `HeapObj::VmClosure` in a heap slot.
    pub closure_tag: usize,
    /// Slot base → the closure's `Rc` control pointer.
    pub closure_payload_off: usize,
    /// `Rc` control pointer → its value.
    pub rc_value_off: usize,
    /// `Rc` control pointer → its strong count.
    pub rc_strong_off: usize,
    /// `VmClosure` → its proto's `Rc` control pointer.
    pub closure_proto_off: usize,
    /// `FunctionProto` → `jit_native`, `jit_native_sig`, `jit_epoch`.
    pub proto_native_off: usize,
    pub proto_native_sig_off: usize,
    pub proto_epoch_off: usize,
    /// `ExecCtx` → the frame `Vec`'s data pointer, length and capacity.
    pub frames_ptr_off: usize,
    pub frames_len_off: usize,
    pub frames_cap_off: usize,
    /// `CallFrame` size and fields.
    pub frame_size: usize,
    pub frame_closure_ptr_off: usize,
    pub frame_owned_off: usize,
    pub frame_ip_off: usize,
    pub frame_base_off: usize,
    pub frame_class_off: usize,
    pub frame_return_reg_off: usize,
    /// `ClassObj` value → its vtable `Vec`'s data pointer and length, and
    /// `vtable_version`.
    pub class_vtable_ptr_off: usize,
    pub class_vtable_len_off: usize,
    pub class_vtable_version_off: usize,
    pub no_activation: usize,
    pub no_return_reg: usize,
    pub max_call_depth: usize,
}

/// Home-slot addressing for the frame-aware lowering (`homes.rs`): the four
/// `FrameStore` class-vector data pointers plus the activation-bases vector.
/// The only `Vec` walks left on the hot path (data-pointer reload after a
/// call/safepoint that may reallocate); everything else `emit_vm_call` used
/// to hand-roll is gone with it.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct JitFrameLayout {
    /// Byte offsets from the `ExecCtx` base to the DATA-POINTER word of each
    /// `FrameStore` class vector, indexed by `SlotClass::index()`
    /// (0=gpr i64, 1=fpr f64, 2=refs u32, 3=dyn VmValue). Generated code
    /// reloads these after any call/safepoint that can push a frame and
    /// reallocate a vector; the per-activation class bases come from the ABI
    /// (the `FrameStore` activation id), not from here.
    pub gpr_ptr_offset: usize,
    pub fpr_ptr_offset: usize,
    pub refs_ptr_offset: usize,
    pub dyn_ptr_offset: usize,
    /// Byte offset from the `ExecCtx` base to the DATA-POINTER word of
    /// `FrameStore::allocs` (`Vec<FrameAlloc>`). Generated code reads an
    /// activation's per-class base indices as
    /// `allocs[act_id].bases[class]`, i.e. at
    /// `allocs_ptr + act_id * alloc_size + alloc_bases_offset + class*4`.
    pub allocs_ptr_offset: usize,
    /// `size_of::<FrameAlloc>()` — stride between activations in `allocs`.
    pub alloc_size: usize,
    /// Byte offset of `FrameAlloc::bases` within a `FrameAlloc` (`0` by
    /// `#[repr(C)]`, asserted at probe time).
    pub alloc_bases_offset: usize,
}

macro_rules! define_tail {
    ( $( $field:ident ),* $(,)? ) => {
        #[derive(Debug, Clone, Copy)]
        #[repr(C)]
        pub struct JitHelpers {
            $( pub $field: usize, )*
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
        /// Byte offset of the heap field (an Rc, i.e. one pointer) inside ExecCtx.
        pub heap_field_offset: usize,
        /// Byte offset from the heap RcBox pointer to the young-birth count.
        pub young_len_offset: usize,
        /// Young births at which the safepoint must run a minor collection.
        pub young_threshold: usize,
        pub jit_native_result_offset: usize,
        pub jit_exit_offset: usize,
        /// Byte offset of the `globals` field (an Rc, i.e. one pointer) inside
        /// ExecCtx. The store itself is shared across task forks; chase it
        /// with `globals_store_offset` (same two-link shape as the heap
        /// RcBox in `emit_gc_poll`).
        pub globals_offset: usize,
        /// Byte offset from the globals RcBox pointer to the `values` buffer:
        /// Rc control prefix + `values` field + Vec buffer word.
        pub globals_store_offset: usize,
        /// Byte offset of `module_base: u32` within `VmClosure`. A `LoadGlobalIdx`
        /// slot is relative to the running closure's module region; the lowering
        /// loads this from the closure param and adds it. `LoadNativeGlobalIdx`
        /// is absolute and ignores it.
        pub closure_module_base_offset: usize,
        /// Byte offset of `ic_entries: *const PolyICSlot` within `VmClosure` —
        /// the poly inline-cache vec's data pointer, so a lowering can probe
        /// entry `[cs]` inline (`base + cs * poly_ic_slot_size`).
        pub closure_ic_entries_offset: usize,
        /// `size_of::<PolyICSlot>()`, the stride between poly slots.
        pub poly_ic_slot_size: usize,
        pub frame_layout: JitFrameLayout,
        pub call_layout: JitCallLayout,
        }
    };
}

jit_helper_table! { define, "../varn-vm/src/exec/jit_helpers" }

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
/// Cranelift is asked — unless it is leaf-safe (no nesting possible).
/// Enforced once, inside `clif::lower::try_compile`, so production and the
/// debug passes gate identically; see `lower::gate_reason`.
pub const SIZE_GATE_WORDS: usize = 8192;

fn fn_name(proto: &FunctionProto) -> String {
    proto.name.as_deref().unwrap_or("<module>").to_owned()
}

/// What a successful compilation hands back to the VM: the wrapper entry, and
/// the native entry with its ABI shape (`0`/`0` for a framed body), which the
/// VM publishes in `FunctionProto::jit_native` for other call sites to load.
pub struct Compiled {
    pub entry: JitFn,
    pub native: usize,
    pub native_sig: u64,
    pub code: Rc<dyn Any>,
}

/// Whether the CLIF backend is held shut for the two-word value migration.
///
/// `VmValue` is now a tag word plus a payload word. The interpreter, the heap
/// and the host boundary carry both; this backend does not — every VM register
/// here is a single Cranelift `Variable` of type `I64` holding a whole
/// NaN-boxed value, an assumption baked into `use_boxed`, the box/unbox
/// primitives, the stack home-slot addressing and every runtime-helper
/// signature.
///
/// Rather than ship a lowering that quietly reads half a value, the backend
/// declines. Everything runs interpreted meanwhile: correct, and slower on the
/// hot paths. The primitives that must change are marked with
/// `unimplemented!("… awaits the two-word value migration")`; when the last of
/// them is gone this flips to `false`.
///
/// What the migration has to do, in order:
/// 1. `K` boxed kinds carry two `Variable`s; raw `Int`/`Float`/`Bool` keep one.
/// 2. Home-slot addressing scales by `size_of::<VmValue>()`, not by 8.
/// 3. Helper signatures take/return the pair. On Windows x64 a 16-byte struct
///    returns through a hidden pointer, so the return convention differs from
///    SysV and must be declared per target.
/// 4. The inline fast paths (fields, arrays, SSO, strconcat) test the tag word
///    and read the payload word, instead of masking one word.
pub(crate) const PAIR_MIGRATION_PENDING: bool = false;

/// The bail reason reported while [`PAIR_MIGRATION_PENDING`] stands. Shows up
/// in `vn debug -p bails` and in `VARN_CLIF_TRACE=1`, so the reason a function
/// is interpreted is never a mystery.
pub(crate) const PAIR_MIGRATION_BAIL: &str =
    "clif: disabled while `VmValue` moves to a (tag, payload) pair; the lowering still models every register as one machine word";

/// Lower `proto` for the running context.
///
/// `osr_ip` picks the entry shape. `None` is the ordinary one. `Some(ip)`
/// builds an ON-STACK REPLACEMENT entry: same body, but a parameterless
/// prologue that reloads the register file from the live frame and resumes at
/// `ip`, so a function that was entered once and is still looping can be
/// compiled without waiting for a second entry that may never come. Such a
/// lowering is always frame-aware, so [`Compiled::raw`] comes back `0` and no
/// call site can reach the resume prologue.
///
/// Gating (size, coroutines) and the lowering itself both live in
/// `clif::lower::try_compile`, which this and the debug passes reach through
/// the same choke point — including the `PAIR_MIGRATION_PENDING` gate, which
/// is NOT repeated here for that reason.
pub fn compile(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: JitHelpers,
    linker: &dyn clif::lower::ClifLinker,
    osr_ip: Option<usize>,
) -> Result<Compiled, String> {
    // Everything routes through the Cranelift backend; a bail leaves the
    // function to the interpreter.
    if clif::enabled() {
        if let Ok(isa) = clif::shared_isa() {
            let start = std::time::Instant::now();
            let res =
                clif::lower::try_compile(proto, constants, &helpers, isa, linker, osr_ip, None);
            let elapsed = start.elapsed().as_nanos() as u64;
            let words = proto.chunk.code.len();
            match res {
                Ok(art) => {
                    if clif::trace() {
                        eprintln!("CLIF ROUTE {:?}", proto.name);
                    }
                    JIT_STATS.compile_success.fetch_add(1, Ordering::Relaxed);
                    JIT_STATS
                        .total_compile_time_ns
                        .fetch_add(elapsed, Ordering::Relaxed);
                    JIT_STATS
                        .total_code_size_bytes
                        .fetch_add(art.buffer.size() as u64, Ordering::Relaxed);
                    record(|| CompileRecord {
                        name: fn_name(proto),
                        words,
                        outcome: CompileOutcome::Routed,
                        compile_ns: elapsed,
                    });
                    let jit_fn: JitFn = unsafe { std::mem::transmute(art.entry) };
                    let (native, native_sig) = art.native_entry(proto);
                    return Ok(Compiled {
                        entry: jit_fn,
                        native,
                        native_sig,
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
