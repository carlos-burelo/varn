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

/// Finalized machine code for one function's buffer (raw fn at `raw_off`,
/// wrapper at `entry_off`).
#[derive(Debug, Default, Clone)]
pub struct CodeBytes {
    pub bytes: Vec<u8>,
    pub raw_off: usize,
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

/// Record the finalized code bytes (raw fn at 0, wrapper at `wrapper_off`).
pub(super) fn capture_code(
    debug: &mut Option<&mut ClifDebugSink>,
    buf: &mut JitBuffer,
    wrapper_off: usize,
) {
    if let Some(sink) = debug.as_deref_mut() {
        sink.code = Some(CodeBytes {
            bytes: buf.as_mut_slice().to_vec(),
            raw_off: 0,
            entry_off: wrapper_off,
        });
    }
}
