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

/// Capture slots populated by `try_compile` when inspection is active.
#[derive(Debug, Default)]
pub struct ClifDebugSink {
    pub kinds: Option<KindReport>,
    pub clif_ir: Option<String>,
    pub code: Option<CodeBytes>,
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

use varn_types::{FunctionProto, VmValue};
use cranelift_codegen::isa::OwnedTargetIsa;
use crate::JitHelpers;
use super::lower::{try_compile, ClifLinker};

/// Everything `vn debug -p clif` shows for one function.
pub struct ClifInspection {
    pub name: String,
    /// `Ok(())` = ROUTE; `Err(reason)` = BAIL.
    pub route: Result<(), String>,
    pub kinds: Option<KindReport>,
    pub clif_ir: Option<String>,
    pub code: Option<CodeBytes>,
    pub frame_aware: bool,
}

/// Run the clif lowering for `proto` with capture active, without executing.
pub fn inspect(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    linker: &dyn ClifLinker,
) -> ClifInspection {
    let mut sink = ClifDebugSink::default();
    let result = try_compile(proto, constants, helpers, isa, linker, Some(&mut sink));
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
    }
}
