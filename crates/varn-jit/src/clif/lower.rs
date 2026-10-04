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
use varn_types::{FunctionProto, VmValue};

use super::abi::{build_wrapper, Activation};
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
    /// Which ABI `raw` takes; only a native one is callable compiled→compiled.
    pub activation: Activation,
    /// The native body never touches the VM frame, so a caller may enter it
    /// without pushing one.
    pub frameless: bool,
}

impl ClifArtifact {
    /// The native entry and its published shape, or `(0, 0)` when the body
    /// is framed or its shape has no id.
    pub fn native_entry(&self, proto: &FunctionProto) -> (usize, u64) {
        if self.activation != Activation::Native {
            return (0, 0);
        }
        let Some(id) = super::native_abi::NativeShape::of_proto(proto).id() else {
            return (0, 0);
        };
        let frameless = if self.frameless {
            super::native_abi::NATIVE_FRAMELESS
        } else {
            0
        };
        (self.raw as usize, id | frameless)
    }
}

impl Drop for ClifArtifact {
    fn drop(&mut self) {
        crate::stack_roots::unregister(self.raw as usize);
    }
}

/// What a compilation needs from the live VM. Implemented over the running
/// `ExecCtx` (the JIT crate cannot see it).
pub trait ClifLinker {
    /// The VM epoch THIS compilation is happening under, baked into the body
    /// as the callee-epoch guard of every native call: compiled code bakes
    /// handles into one heap, so a callee built for another must not run.
    /// `0` — `NoLinker`'s answer — never matches a published callee, so the
    /// guard always declines.
    fn current_epoch(&self) -> u64;
}

/// Linker with no VM context.
pub struct NoLinker;
impl ClifLinker for NoLinker {
    fn current_epoch(&self) -> u64 {
        0
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
    if proto.resumes_in_interpreter() {
        r.push("resume");
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
    let lowered = match super::from_ssa::try_lower(
        proto,
        ssa,
        constants,
        helpers,
        isa,
        linker,
        osr_ip,
        Activation::Native,
        debug.as_deref_mut(),
    ) {
        Ok(lowered) => Ok((lowered, Activation::Native)),
        Err(reason) if reason == super::from_ssa::NEEDS_ACTIVATION => {
            super::emit::reset_disabled_helper_hit();
            super::from_ssa::try_lower(
                proto,
                ssa,
                constants,
                helpers,
                isa,
                linker,
                osr_ip,
                Activation::Framed,
                debug.as_deref_mut(),
            )
            .map(|lowered| (lowered, Activation::Framed))
        }
        Err(reason) => Err(reason),
    };
    match lowered {
        Ok((lowered, activation)) if !super::emit::disabled_helper_hit() => {
            if super::trace() {
                eprintln!(
                    "clif: from_ssa {}{} ({activation:?}{})",
                    proto.name.as_deref().unwrap_or("<module>"),
                    osr_ip.map_or(String::new(), |ip| format!(" osr@{ip}")),
                    if lowered.frameless { ", frameless" } else { "" }
                );
            }
            let wrapper = build_wrapper(proto, helpers, isa, activation, osr_ip.is_some())?;
            finish_artifact(lowered, wrapper, activation, debug)
        }
        Ok(_) => Err("clif: uses a helper disabled in fase B".into()),
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
    lowered: super::from_ssa::Lowered,
    wrapper: super::piece::CompiledPiece,
    activation: Activation,
    mut debug: Option<&mut ClifDebugSink>,
) -> Result<ClifArtifact, String> {
    let raw = lowered.piece;
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
    let mut safepoints = raw.safepoints;
    for mut site in wrapper.safepoints {
        site.return_offset += wrapper_off as u32;
        safepoints.push(site);
    }
    crate::stack_roots::register(raw_ptr as usize, total, safepoints);
    let entry = unsafe { buf.as_ptr().add(wrapper_off) };
    Ok(ClifArtifact {
        buffer: buf,
        entry,
        raw: raw_ptr,
        activation,
        frameless: lowered.frameless,
    })
}
