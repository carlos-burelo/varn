//! VM side of the Cranelift static-call linker.
//!
//! When a function clif-compiles a cross-function `Call`, it asks this
//! linker which closure the global slot holds, so the call site can bind
//! directly to that closure's proto. The answer is derived from the live
//! `ExecCtx`: clif compilation happens during execution, so a thread-local
//! records the executing context for the duration of a run.
//!
//! The link does NOT require the callee to be compiled yet. Callers reach
//! their tier threshold before their callees do — a caller must be entered
//! for its callee to be entered at all — so demanding compiled code here
//! would decline essentially every call and never revisit it. Instead the
//! link carries the ADDRESS of the callee proto's `clif_raw` cell, which the
//! call site loads at run time: `0` until the callee compiles, the direct
//! entry afterwards.
//!
//! Every link is only a runtime HINT — the generated call site guards on
//! the callee's exact `VmValue` bits and on a non-zero entry, taking the
//! interpreter fallback on any mismatch (rebind, GC move, uncompiled) — so a
//! stale or wrong context here can only cost speed, never correctness.

use std::cell::Cell;

use varn_jit::clif::lower::{ClifLinker, ClifTarget};

use crate::exec::ExecCtx;

thread_local! {
    static CURRENT_CTX: Cell<*const ExecCtx> = const { Cell::new(std::ptr::null()) };
}

/// Records `ctx` as the linking context for the duration of the guard,
/// restoring the previous one on drop (so nested runs compose).
pub struct CtxGuard(*const ExecCtx);

impl CtxGuard {
    pub fn enter(ctx: *const ExecCtx) -> Self {
        let prev = CURRENT_CTX.with(|c| c.replace(ctx));
        CtxGuard(prev)
    }
}

impl Drop for CtxGuard {
    fn drop(&mut self) {
        CURRENT_CTX.with(|c| c.set(self.0));
    }
}

/// Linker bound to whatever context is current on this thread.
pub struct CtxLinker(*const ExecCtx);

impl CtxLinker {
    pub fn current() -> Self {
        CtxLinker(CURRENT_CTX.with(|c| c.get()))
    }
}

impl ClifLinker for CtxLinker {
    fn static_target(&self, global_idx: usize) -> Option<ClifTarget> {
        if self.0.is_null() {
            return None;
        }
        // Safety: the pointer is valid for the lifetime of the CtxGuard that
        // set it; clif compilation runs synchronously inside that run.
        let ctx = unsafe { &*self.0 };
        let gv = *ctx.globals.values.get(global_idx)?;
        if !gv.is_heap() {
            return None;
        }
        let closure = match ctx.heap.get(gv.as_heap_idx()) {
            Some(crate::heap::HeapObj::VmClosure(c)) => c,
            _ => return None,
        };
        let proto = &closure.proto;
        // The proto lives in an `Rc` for as long as any closure over it does,
        // and compiled code outlives neither — so the cell's address stays
        // valid for the lifetime of every call site that embeds it. The cell
        // stays `0` for a proto that fails to compile or takes the
        // frame-aware lowering; the call site checks it on every call.
        Some(ClifTarget {
            raw_slot: &proto.clif_raw as *const std::cell::Cell<usize> as usize,
            expected_bits: gv.0,
            param_kinds: proto.param_kinds.clone(),
            return_kind: proto.return_kind,
        })
    }
}
