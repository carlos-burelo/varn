//! Static introspection of the Cranelift lowering for `vn debug -p clif`.
//!
//! Reuses `try_compile` as the single source of truth: a `ClifDebugSink`
//! is threaded in and the lowering records the kind lattice, the textual
//! CLIF IR, and the finalized code bytes as it goes. The production path
//! passes `None`, so this adds nothing to normal compilation.

/// Kind lattice at each block entry (from `kind_flow`). `blocks[i] =
/// (block_start_ip, [K per register as text])`.
#[derive(Debug, Default, Clone)]
pub struct KindReport {
    pub nregs: usize,
    pub blocks: Vec<(usize, Vec<String>)>,
}

/// Finalized machine code for one function's buffer (raw fn at `raw_off`
/// spanning `raw_len` bytes, wrapper at `entry_off`). The two are separate
/// code ranges with up-to-15 bytes of alignment padding between them; a
/// disassembler must decode each range independently or the padding desyncs
/// it.
#[derive(Debug, Default, Clone)]
pub struct CodeBytes {
    pub bytes: Vec<u8>,
    pub raw_off: usize,
    pub raw_len: usize,
    pub entry_off: usize,
}

/// One safepoint's two answers to "what is live here".
///
/// `ours` is the set `alloc::live_boxed` actually flushed to home slots, so it
/// is what the collector can see today. `cranelift` is how many of the same
/// registers Cranelift's own SSA liveness kept in the stack map at the
/// correlated PC. Two independent analyses of one program point:
///
/// * `ours` bigger — we flush registers Cranelift proved dead. Safe, and the
///   difference is wasted stores.
/// * `cranelift` bigger — Cranelift believes something is live that we do not
///   root. That is the shape of a missing GC root, EXCEPT for the registers in
///   `unboxed`: those the kind flow proved hold a raw machine integer, so
///   Cranelift keeping them alive is correct and rooting them would be
///   meaningless. Cranelift marks every I64 Variable and cannot see our kinds,
///   so the comparison has to net them out.
#[derive(Debug, Clone)]
pub struct SafepointRoots {
    pub ip: usize,
    pub op: String,
    pub ours: Vec<usize>,
    /// Live here, deliberately NOT flushed: the kind flow typed them `Int` or
    /// `Bool`, which cannot carry a heap index.
    pub unboxed: Vec<usize>,
    /// `None` when no stack map correlated to this ip — itself a finding, it
    /// means the safepoint emitted no map at all.
    pub cranelift: Option<usize>,
}

/// Per-function result of `vn debug -p roots`.
#[derive(Debug, Default, Clone)]
pub struct RootsReport {
    pub points: Vec<SafepointRoots>,
    pub maps_total: usize,
    /// Stack maps whose PC fell outside every recorded safepoint's srcloc.
    pub maps_unmatched: usize,
}

/// Capture slots populated by `try_compile` when inspection is active.
#[derive(Debug, Default)]
pub struct ClifDebugSink {
    pub kinds: Option<KindReport>,
    pub clif_ir: Option<String>,
    pub code: Option<CodeBytes>,
    /// Requested by `-p roots` BEFORE inspecting. It turns on stack-map
    /// marking and per-opcode srclocs, both of which change what Cranelift
    /// emits, so it stays off for `-p clif` — that pass has to show what
    /// production actually compiles.
    pub want_roots: bool,
    pub roots: Option<RootsReport>,
}

use std::collections::HashMap;

use cranelift_codegen::ir::Function;

use super::kinds::K;
use crate::mem::JitBuffer;

/// Record the kind lattice into an active sink.
pub(super) fn capture_kinds(
    debug: &mut Option<&mut ClifDebugSink>,
    entries: &HashMap<usize, Vec<K>>,
    nregs: usize,
) {
    if let Some(sink) = debug.as_deref_mut() {
        let mut blocks: Vec<(usize, Vec<String>)> = entries
            .iter()
            .map(|(start, ks)| (*start, ks.iter().map(|k| format!("{k:?}")).collect()))
            .collect();
        blocks.sort_by_key(|(s, _)| *s);
        sink.kinds = Some(KindReport { nregs, blocks });
    }
}

/// Record the textual CLIF IR of the raw function.
#[allow(dead_code)]
pub(super) fn capture_ir(debug: &mut Option<&mut ClifDebugSink>, func: &Function) {
    if let Some(sink) = debug.as_deref_mut() {
        sink.clif_ir = Some(func.display().to_string());
    }
}

/// Record the finalized code bytes: raw fn at 0 spanning `raw_len` bytes,
/// wrapper at `wrapper_off` (16-aligned, so `[raw_len, wrapper_off)` is padding),
/// wrapper ending at `total`. The buffer is page-rounded, so the capture is
/// truncated to `total` — everything past it is zero-fill page tail, not code.
pub(super) fn capture_code(
    debug: &mut Option<&mut ClifDebugSink>,
    buf: &mut JitBuffer,
    raw_len: usize,
    wrapper_off: usize,
    total: usize,
) {
    if let Some(sink) = debug.as_deref_mut() {
        let slice = buf.as_mut_slice();
        let end = total.min(slice.len());
        sink.code = Some(CodeBytes {
            bytes: slice[..end].to_vec(),
            raw_off: 0,
            raw_len,
            entry_off: wrapper_off,
        });
    }
}

/// Join the two root answers for one function.
///
/// Both sides are grouped by bytecode ip, because one opcode can emit more
/// than one safepoint (a call that flushes, plus a loop back-edge check).
/// `ours` is the union of everything flushed at that ip and `cranelift` the
/// largest map emitted there — the pairing that cannot understate either side.
pub(super) fn capture_roots(
    debug: &mut Option<&mut ClifDebugSink>,
    safepoints: &[(usize, Vec<usize>, Vec<usize>)],
    stack_maps: &[(usize, usize)],
    maps_unmatched: usize,
    op_name: impl Fn(usize) -> String,
) {
    let Some(sink) = debug.as_deref_mut() else {
        return;
    };
    if !sink.want_roots {
        return;
    }
    let mut by_ip: Vec<(usize, Vec<usize>, Vec<usize>)> = Vec::new();
    for (ip, regs, unboxed) in safepoints {
        match by_ip.iter_mut().find(|(i, _, _)| i == ip) {
            Some((_, acc, acc_un)) => {
                for r in regs {
                    if !acc.contains(r) {
                        acc.push(*r);
                    }
                }
                for r in unboxed {
                    if !acc_un.contains(r) {
                        acc_un.push(*r);
                    }
                }
            }
            None => by_ip.push((*ip, regs.clone(), unboxed.clone())),
        }
    }
    by_ip.sort_by_key(|(ip, _, _)| *ip);

    let points = by_ip
        .into_iter()
        .map(|(ip, mut ours, mut unboxed)| {
            ours.sort_unstable();
            unboxed.sort_unstable();
            SafepointRoots {
                ip,
                op: op_name(ip),
                ours,
                unboxed,
                cranelift: stack_maps
                    .iter()
                    .filter(|(i, _)| *i == ip)
                    .map(|(_, n)| *n)
                    .max(),
            }
        })
        .collect();

    sink.roots = Some(RootsReport {
        points,
        maps_total: stack_maps.len() + maps_unmatched,
        maps_unmatched,
    });
}

use super::lower::{try_compile, ClifLinker};
use crate::JitHelpers;
use cranelift_codegen::isa::OwnedTargetIsa;
use varn_types::{FunctionProto, VmValue};

/// Everything `vn debug -p clif` shows for one function.
pub struct ClifInspection {
    pub name: String,
    /// `Ok(())` = ROUTE; `Err(reason)` = BAIL.
    pub route: Result<(), String>,
    pub kinds: Option<KindReport>,
    pub clif_ir: Option<String>,
    pub code: Option<CodeBytes>,
    pub frame_aware: bool,
    /// Which tests made it frame-aware — see `lower::frame_aware_reasons`.
    pub fa_reasons: Vec<&'static str>,
    /// Populated only when the caller asked for roots — see `inspect_roots`.
    pub roots: Option<RootsReport>,
}

/// Run the clif lowering for `proto` with capture active, without executing.
pub fn inspect(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    linker: &dyn ClifLinker,
) -> ClifInspection {
    inspect_with(proto, constants, helpers, isa, linker, false)
}

/// [`inspect`] with stack-map marking on, for `vn debug -p roots`. Separate
/// entry point because the marking is not free: Cranelift spills every marked
/// value around every safepoint, so this compiles something production does
/// not, and only this pass may ask for it.
pub fn inspect_roots(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    linker: &dyn ClifLinker,
) -> ClifInspection {
    inspect_with(proto, constants, helpers, isa, linker, true)
}

fn inspect_with(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    linker: &dyn ClifLinker,
    want_roots: bool,
) -> ClifInspection {
    let mut sink = ClifDebugSink {
        want_roots,
        ..Default::default()
    };
    let result = try_compile(
        proto,
        constants,
        helpers,
        isa,
        linker,
        None,
        Some(&mut sink),
    );
    let (route, frame_aware) = match &result {
        Ok(art) => (Ok(()), art.frame_aware),
        Err(e) => (Err(e.clone()), false),
    };
    ClifInspection {
        name: proto.name.as_deref().unwrap_or("<top-level>").to_string(),
        route,
        kinds: sink.kinds,
        clif_ir: sink.clif_ir,
        code: sink.code,
        frame_aware,
        fa_reasons: super::lower::frame_aware_reasons(proto),
        roots: sink.roots,
    }
}
