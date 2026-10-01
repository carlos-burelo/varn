//! Bytecode → CLIF lowering: public surface and the compilation pipeline.
//!
//! Lowered from BYTECODE, not from the compiler's SSA: cached `.vnc` runs
//! only have bytecode, and the typed opcode variants (`AddInt`, `LtInt`, …)
//! ARE the checker's serialized proofs. `cranelift-frontend` Variables (one
//! per VM register, all `I64`) rebuild SSA for free.
//!
//! Two functions per compilation, one buffer:
//! * the RAW function — unboxed `fn(i64 × nparams) -> i64`, the entire body
//!   in native registers, recursion as a direct hardware call to its own
//!   entry;
//! * the WRAPPER — the template JIT's `JitFn` ABI. The caller always pushed
//!   the frame before entry (v2 §1, sin handshake), so the wrapper loads
//!   the boxed args from the VM stack, unboxes their payloads, calls the raw
//!   function and re-tags the result.
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
use super::debug::ClifDebugSink;
use super::emit::patch_rel32;
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

#[derive(Clone, Debug)]
pub struct ClifClassTarget {
    pub class_id: u32,
    pub expected_bits: u64,
    pub payload_size: u32,
    /// Offsets of the class's `Ref` slots. A fresh payload is zero-filled,
    /// so the inline `new` writes the `null` niche into each of them before
    /// the constructor's own stores, as `InstanceData::alloc` does.
    pub ref_slots: Vec<u32>,
    pub trivial_plan: Option<Vec<ClifFieldInit>>,
}

/// One field initialised by a trivial constructor, at its COMPACT layout —
/// `(param, offset, repr)` from the class's `ClassLayout`, so
/// the inline `new X()` path writes the same bytes `InstanceData::write_field`
/// would (`varn-types/src/value/instance.rs`).
#[derive(Clone, Copy, Debug)]
pub struct ClifFieldInit {
    /// Argument register is `arg_start + 1 + param_idx` (the callee placeholder
    /// occupies `arg_start`).
    pub param_idx: usize,
    /// Byte offset from the instance payload start.
    pub offset: u32,
    pub repr: varn_types::layout::ScalarRepr,
}

/// VM-side resolver for clif→clif static calls. Implemented over the live
/// `ExecCtx` at compile time (globals are runtime state, so the JIT crate
/// cannot see them itself).
pub trait ClifLinker {
    fn static_target(&self, global_idx: usize) -> Option<ClifTarget>;
    fn static_class_target(&self, _global_idx: usize) -> Option<ClifClassTarget> {
        None
    }
    /// The VM epoch THIS compilation is happening under, baked into the
    /// lowered body as an `iconst` for the inline frame-aware `Call` fast
    /// path's callee-epoch guard. `0` —
    /// the default, and `NoLinker`'s only answer — can never match a real
    /// callee's `jit_epoch` (a published `jit_entry` always stamps a nonzero
    /// one), so a linker with no real epoch to report just makes that guard
    /// permanently decline, same as it declining for any other reason.
    fn current_epoch(&self) -> u64 {
        0
    }
}

/// Linker that never links — used by paths without a VM context.
pub struct NoLinker;
impl ClifLinker for NoLinker {
    fn static_target(&self, _global_idx: usize) -> Option<ClifTarget> {
        None
    }
}

/// Whether a proto's ABI forces a frame. Only the SIGNATURE matters: a boxed
/// parameter cannot be passed in the raw `i64`-per-arg ABI, and a boxed return
/// cannot be delivered without `exec_ctx` (the raw entry returns `i64`).
fn boxed_slots(proto: &FunctionProto) -> bool {
    use varn_types::register_meta::SlotKind;
    let scalar = |k: &SlotKind| matches!(k, SlotKind::Int | SlotKind::Float | SlotKind::Bool);
    proto.param_kinds.iter().any(|k| !scalar(k)) || !scalar(&proto.return_kind)
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
    if boxed_slots(proto) {
        r.push("boxed");
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

/// Why the size gate refused `proto`, or `None` when it is admitted. The
/// single authority on the gate: production (`try_compile` below) and the
/// debug passes (`-p tiers`, `-p bails`, `-p roots`) all ask here, so the
/// threshold and the leaf-safe exception cannot drift apart.
pub fn gate_reason(proto: &FunctionProto) -> Option<String> {
    let words = proto.chunk.code.len();
    if words > crate::SIZE_GATE_WORDS && !leaf_safe(proto) {
        return Some(format!("too large (>{} words)", crate::SIZE_GATE_WORDS));
    }
    None
}

/// Whether an oversized function may still compile: it cannot nest a clif
/// frame under another (no calls or allocations a throw could unwind
/// through, no suspension points) and it is not a coroutine. An undecodable
/// body counts as unsafe and stays gated.
fn leaf_safe(proto: &FunctionProto) -> bool {
    if proto.is_generator || proto.is_async {
        return false;
    }
    !alloc::has_alloc(&proto.chunk.code, &proto.chunk.constants).unwrap_or(true)
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
    // The single choke point for the lowering: `jit::compile` reaches it and
    // so does `clif::debug::inspect` (`vn debug -p tiers` / `-p clif`), which
    // compiles without executing.
    if crate::PAIR_MIGRATION_PENDING {
        return Err(crate::PAIR_MIGRATION_BAIL.to_owned());
    }

    super::emit::reset_disabled_helper_hit();

    // NOT a compile-time budget: the size gate fires before anything is
    // asked, so a rejected function shows up in neither `CLIF BAIL` nor
    // `compile_fail`. Counting "0 bails" without it overstates coverage.
    if let Some(reason) = gate_reason(proto) {
        if super::trace() {
            eprintln!("CLIF GATE  {:?}: {reason}", proto.name);
        }
        crate::stats::JIT_STATS
            .gate_rejected
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        crate::stats::record(|| crate::stats::CompileRecord {
            name: crate::fn_name(proto),
            words: proto.chunk.code.len(),
            outcome: crate::stats::CompileOutcome::Gated(reason.clone()),
            compile_ns: 0,
        });
        return Err("JIT Bailout: function too large".to_owned());
    }
    if proto.chunk.code.len() > crate::SIZE_GATE_WORDS && super::trace() {
        eprintln!(
            "CLIF GATE  {:?}: large but leaf-safe, admitted ({} words)",
            proto.name,
            proto.chunk.code.len()
        );
    }

    // Declared generators/async keep the interpreter: their calling
    // convention is a state machine the compiled wrapper does not speak, and
    // no body lowering can fix that. This gate stays ahead of the SSA attempt
    // so they never count as an SSA bail. Top-level `await` (a non-async
    // `<module>` body) is unaffected and lowers from SSA below.
    if proto.is_generator || proto.is_async {
        return Err("clif: generator/async not JIT-able in fase B".into());
    }

    // The one lowering: the portable typed SSA, which covers the whole
    // instruction family (scalars, aggregates, calls, modules, suspension).
    // The debug sink threads through for `-p clif` capture; production passes
    // `None`. An `Err` leaves the function to the interpreter — the fallback
    // is a missing optimization, never a wrong result (Ley 10).
    if let Some(why) = proto.ssa.unavailable() {
        if super::trace() {
            eprintln!(
                "clif: from_ssa unavailable {}: {why}",
                proto.name.as_deref().unwrap_or("<module>")
            );
        }
        return Err(format!("clif: from_ssa unavailable: {why}"));
    }
    let Some(ssa) = proto.ssa.get() else {
        return Err("clif: from_ssa unavailable".into());
    };
    match super::from_ssa::try_lower(
        proto,
        ssa,
        constants,
        helpers,
        isa,
        linker,
        osr_ip,
        debug.as_deref_mut(),
    ) {
        Ok((raw, frame_aware)) if !super::emit::disabled_helper_hit() => {
            if super::trace() {
                eprintln!(
                    "clif: from_ssa {}{}{}",
                    proto.name.as_deref().unwrap_or("<module>"),
                    osr_ip.map_or(String::new(), |ip| format!(" osr@{ip}")),
                    if frame_aware { " (frame-aware)" } else { "" }
                );
            }
            let wrapper = build_wrapper(proto, helpers, isa, frame_aware, osr_ip.is_some())?;
            finish_artifact(raw, wrapper, frame_aware, debug)
        }
        Ok(_) => {
            Err("clif: uses a helper disabled in fase B".into())
        }
        Err(reason) => {
            if super::trace() {
                eprintln!(
                    "clif: from_ssa bail {}: {reason}",
                    proto.name.as_deref().unwrap_or("<module>")
                );
            }
            Err(reason)
        }
    }
}

/// Concatenate the two pieces (raw at 0, wrapper 16-aligned after it), resolve
/// the only two relocation targets admitted (self-recursion inside raw and the
/// wrapper's call to raw) by hand, and hand back the executable artifact.
fn finish_artifact(
    raw: super::piece::CompiledPiece,
    wrapper: super::piece::CompiledPiece,
    frame_aware: bool,
    mut debug: Option<&mut ClifDebugSink>,
) -> Result<ClifArtifact, String> {
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
