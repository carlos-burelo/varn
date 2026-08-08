//! Bytecode → CLIF lowering: public surface and the compilation pipeline.
//!
//! Lowered from BYTECODE, not from the compiler's SSA: cached `.vnc` runs
//! only have bytecode, and the typed opcode variants (`AddInt`, `LtInt`, …)
//! ARE the checker's serialized proofs. `cranelift-frontend` Variables (one
//! per VM register, all `I64`) rebuild SSA for free.
//!
//! Two functions per compilation, one buffer:
//! * the RAW function — unboxed `fn(i64 × nparams) -> i64`, the entire body
//!   in native registers, i48 wrap after every arith op, recursion as a
//!   direct hardware call to its own entry;
//! * the WRAPPER — the template JIT's `JitFn` ABI. It clears the
//!   caller-prepush flag (protocol: every JIT prologue consumes it), loads
//!   the boxed args from the VM stack, sign-extends the i48 payloads, calls
//!   the raw function and re-tags the result.
//!
//! Anything outside the supported subset bails, and a bail leaves the
//! function to the interpreter — two tiers, one authority.
//!
//! This module owns the artifact types, the linker seam, and [`try_compile`],
//! which is the pipeline: scan → lower body → build wrapper → concatenate and
//! relocate. The stages themselves live next door:
//!
//! | Stage | Module |
//! |---|---|
//! | CFG scan, loop-region plan | [`super::scan`] |
//! | Body walk + opcode dispatch | [`super::body`] |
//! | Raw signature, `JitFn` wrapper | [`super::abi`] |
//! | On-stack-replacement prologue | [`super::osr`] |
//! | Cranelift invocation, relocs, stack maps | [`super::piece`] |
//!
//! v1 limitations (documented, suite-gated): no native stack-limit guard
//! (deep CallSelf recursion aborts instead of raising the VM's depth
//! error), and back-edges carry no GC safepoint unless the function
//! allocates — sound because a non-allocating routed function cannot create
//! GC pressure.

use cranelift_codegen::isa::OwnedTargetIsa;
use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::register_meta::SlotKind;
use varn_types::{FunctionProto, VmValue};

use super::abi::build_wrapper;
use super::alloc;
use super::body::lower_raw;
use super::debug::ClifDebugSink;
use super::emit::patch_rel32;
use super::floats;
use crate::mem::JitBuffer;
use crate::JitHelpers;

/// Compiled artifact: `entry` (the wrapper, `JitFn` ABI) and `raw` (the
/// unboxed body, callable clif→clif) both point into `buffer`, which lives
/// as long as the owning `FunctionProto` — raw addresses handed to other
/// compilations stay valid.
pub struct ClifArtifact {
    pub buffer: JitBuffer,
    pub entry: *const u8,
    pub raw: *const u8,
    /// Whether `raw` takes the frame-aware ABI (extra base+closure params).
    /// Such a function must NOT be called through the clif→clif fast path
    /// (which assumes the bare `(exec_ctx, args)` ABI); the linker rejects it
    /// so the call takes the wrapper-based fallback instead.
    pub frame_aware: bool,
}

/// A statically linkable call target: the CURRENT closure a global slot
/// holds, bound to that closure's proto.
pub struct ClifTarget {
    /// Address of the callee proto's `clif_raw` cell — NOT the entry itself.
    /// The call site loads it on every call, so a callee compiled after its
    /// caller still gets called directly. `0` means "no direct entry yet"
    /// (uncompiled, failed, or frame-aware) and selects the fallback.
    pub raw_slot: usize,
    /// The exact boxed `VmValue` bits of the closure the link was made
    /// against. The call site guards on equality: a rebound (or GC-moved)
    /// global mismatches and takes the generic fallback — never a wrong
    /// call, at worst a slow one.
    pub expected_bits: u64,
    pub param_kinds: Vec<SlotKind>,
    pub return_kind: SlotKind,
}

/// VM-side resolver for clif→clif static calls. Implemented over the live
/// `ExecCtx` at compile time (globals are runtime state, so the JIT crate
/// cannot see them itself).
pub trait ClifLinker {
    fn static_target(&self, global_idx: usize) -> Option<ClifTarget>;
}

/// Linker that never links — used by paths without a VM context.
pub struct NoLinker;
impl ClifLinker for NoLinker {
    fn static_target(&self, _global_idx: usize) -> Option<ClifTarget> {
        None
    }
}

/// Why a lowering came out frame-aware, in the order the flag tests them,
/// plus `resume` when the body can hand control back to the INTERPRETER at a
/// bytecode ip (`Try`'s catch, `Yield`/`Await`'s suspension), which is what
/// reads registers back out of the home slots.
///
/// `frame_aware` is what zeroes `clif_raw` and so denies a function the direct
/// clif→clif entry. Sizing that gap needs the split: a function marked only
/// `alloc` is one a stack-map rooting model could set free, whereas one that
/// also says `resume` needs its home slots regardless.
pub fn frame_aware_reasons(proto: &FunctionProto) -> Vec<&'static str> {
    let code = &proto.chunk.code;
    let pool = &proto.chunk.constants;
    let mut r = Vec::new();
    if proto.has_this {
        r.push("this");
    }
    if alloc::has_alloc(code, pool).unwrap_or(true) {
        r.push("alloc");
    }
    if proto.upvalue_count > 0 {
        r.push("upvalue");
    }
    if proto.is_generator {
        r.push("generator");
    }
    if proto.is_async {
        r.push("async");
    }
    let mut ip = 0usize;
    while ip < code.len() {
        let Some(info) = decode(code, ip, pool) else {
            break;
        };
        if matches!(
            OpCode::from_u8(code[ip] as u8),
            Some(OpCode::Try)
                | Some(OpCode::Yield)
                | Some(OpCode::Await)
                | Some(OpCode::LoadModule)
        ) {
            r.push("resume");
            break;
        }
        ip += info.len;
    }
    r
}

/// Lower `proto`. `osr_ip` selects the ENTRY, not the body: `None` builds the
/// ordinary entry (arguments in registers, execution from ip 0), `Some(ip)`
/// builds an on-stack-replacement entry that takes no arguments, reloads the
/// register file from the frame's home slots and resumes at `ip`. The body
/// lowered is identical either way — see `clif::osr`.
pub fn try_compile(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    linker: &dyn ClifLinker,
    osr_ip: Option<usize>,
    mut debug: Option<&mut ClifDebugSink>,
) -> Result<ClifArtifact, String> {
    let nparams = proto.arity.saturating_sub(1);
    if proto.param_kinds.len() != nparams {
        return Err("clif: missing param kinds".into());
    }
    floats::check_float_writes(
        &proto.chunk.code,
        &proto.chunk.constants,
        &proto.register_meta,
    )?;

    let has_alloc = alloc::has_alloc(&proto.chunk.code, &proto.chunk.constants)?;
    // OSR resumes a frame that already exists, and reads every register out of
    // that frame's `ctx.stack` home slots — so it needs `base` and `closure`
    // whatever the normal heuristic decided. Forcing the flag also keeps the
    // OSR `raw` out of `clif_raw`: `compile` publishes a direct clif→clif
    // entry only for non-frame-aware lowerings, and a raw that resumes
    // mid-loop is the last thing a call site should reach.
    let frame_aware = osr_ip.is_some()
        || proto.has_this
        || has_alloc
        || proto.upvalue_count > 0
        || proto.is_generator
        || proto.is_async;
    let raw = lower_raw(
        proto,
        constants,
        helpers,
        isa,
        linker,
        has_alloc,
        osr_ip,
        debug.as_deref_mut(),
    )?;
    let wrapper = build_wrapper(proto, helpers, isa, frame_aware, osr_ip.is_some())?;

    // Concatenate: raw at 0, wrapper 16-aligned after it, then resolve the
    // only two relocation targets we admit (self-recursion inside raw, and
    // the wrapper's call to raw) by hand.
    let wrapper_off = (raw.code.len() + 15) & !15;
    let total = wrapper_off + wrapper.code.len();
    let mut buf = JitBuffer::new(total.max(16))?;
    {
        let slice = buf.as_mut_slice();
        slice[..raw.code.len()].copy_from_slice(&raw.code);
        slice[wrapper_off..wrapper_off + wrapper.code.len()].copy_from_slice(&wrapper.code);
        for r in &raw.call_reloc_offsets {
            patch_rel32(slice, *r, 0);
        }
        for r in &wrapper.call_reloc_offsets {
            patch_rel32(slice, wrapper_off + *r, 0);
        }
    }
    super::debug::capture_code(&mut debug, &mut buf, raw.code.len(), wrapper_off, total);
    buf.make_executable()?;
    let raw_ptr = buf.as_ptr();
    let entry = unsafe { buf.as_ptr().add(wrapper_off) };
    Ok(ClifArtifact {
        buffer: buf,
        entry,
        raw: raw_ptr,
        frame_aware,
    })
}
