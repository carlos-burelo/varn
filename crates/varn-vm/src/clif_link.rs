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

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use rustc_hash::FxHashMap;
use varn_jit::clif::lower::{ClifLinker, ClifTarget};
use varn_types::FunctionProto;

use crate::exec::ExecCtx;

thread_local! {
    static CURRENT_CTX: Cell<*const ExecCtx> = const { Cell::new(std::ptr::null()) };
    /// Epoch of the context in `CURRENT_CTX`. Kept beside it rather than read
    /// through the pointer so the hot `jit_fn` check is one TLS load.
    static CURRENT_EPOCH: Cell<u64> = const { Cell::new(0) };
    /// Per epoch: the protos that compiled under it, and the code buffers it
    /// retired. Both are released when the epoch ends.
    static COMPILED: RefCell<FxHashMap<u64, EpochCode>> =
        RefCell::new(FxHashMap::default());
}

#[derive(Default)]
struct EpochCode {
    /// Protos whose `jit_entry` points at code built for this epoch. Held by
    /// `Rc` so the proto — and therefore the `clif_raw` cell whose ADDRESS
    /// sibling call sites baked — cannot be freed while that code can run.
    protos: Vec<Rc<FunctionProto>>,
    /// Code this epoch built and then replaced. A nested context can recompile
    /// a proto that an outer, still-live clif frame is executing, so the buffer
    /// is retired rather than dropped, and only freed with the epoch.
    retired: Vec<Rc<dyn std::any::Any>>,
}

static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);
static NEXT_SERIAL: AtomicU64 = AtomicU64::new(1);

/// A fresh identity for one heap's compiled code. `0` means "no context",
/// which no compiled entry ever matches.
pub(crate) fn next_epoch() -> u64 {
    NEXT_EPOCH.fetch_add(1, Ordering::Relaxed)
}

/// Monotonic tick, stamped on each compilation and read when a heap is copied.
/// It orders "compiled before this copy" against "compiled after it", which is
/// exactly the line between code a copy inherits and code it must not.
pub(crate) fn compile_serial() -> u64 {
    NEXT_SERIAL.load(Ordering::Relaxed)
}

/// Take the next tick, for an entry that has just been built.
pub(crate) fn stamp_compile_serial() -> u64 {
    NEXT_SERIAL.fetch_add(1, Ordering::Relaxed)
}

/// Can the running heap enter `proto`'s compiled entry?
///
/// Yes when it compiled it. Yes, too, when an ancestor compiled it before this
/// heap was copied off that ancestor: a `deep_clone` duplicates the object
/// table wholesale, so those baked handles name the same things here. The
/// entry is re-stamped to the running epoch on the way through, so this walk
/// happens once per proto per heap rather than once per frame entry.
#[cold]
pub(crate) fn adopt_if_inherited(proto: &Rc<FunctionProto>) -> bool {
    let owner = proto.jit_epoch.get();
    if owner == 0 || proto.jit_entry.get().is_none() {
        return false;
    }
    let epoch = current_epoch();
    if epoch == 0 {
        return false;
    }
    let ctx = CURRENT_CTX.with(|c| c.get());
    if ctx.is_null() {
        return false;
    }
    // Safety: same contract as `CtxLinker` — the guard keeps `ctx` alive.
    let inherits = unsafe { (*ctx).heap.jit_ancestry() }
        .iter()
        .any(|&(ancestor, cutoff)| ancestor == owner && proto.jit_serial.get() <= cutoff);
    if !inherits {
        return false;
    }
    proto.jit_epoch.set(epoch);
    register_compiled(proto, None);
    true
}

/// The epoch of the context executing on this thread, `0` outside a run.
#[inline(always)]
pub(crate) fn current_epoch() -> u64 {
    CURRENT_EPOCH.with(|e| e.get())
}

/// Record that `proto` now holds code built for the running context, retiring
/// whatever code it held before.
pub(crate) fn register_compiled(
    proto: &Rc<FunctionProto>,
    previous: Option<(u64, Rc<dyn std::any::Any>)>,
) {
    let epoch = current_epoch();
    COMPILED.with(|m| {
        let mut m = m.borrow_mut();
        if let Some((old_epoch, old_code)) = previous {
            m.entry(old_epoch).or_default().retired.push(old_code);
        }
        m.entry(epoch).or_default().protos.push(proto.clone());
    });
}

/// Park `code` under `epoch` so it stays mapped until that epoch ends.
pub(crate) fn retire_code(epoch: u64, code: Rc<dyn std::any::Any>) {
    COMPILED.with(|m| m.borrow_mut().entry(epoch).or_default().retired.push(code));
}

/// End `epoch`: strip every entry it compiled and free its buffers. Called when
/// the context that produced them is dropped, so nothing can reach code that
/// was baked against a dead heap.
pub(crate) fn invalidate_epoch(epoch: u64) {
    let entry = COMPILED.with(|m| m.borrow_mut().remove(&epoch));
    let Some(entry) = entry else { return };
    for proto in &entry.protos {
        // The two entries carry their own epochs and are cleared
        // independently: a proto can hold a normal entry recompiled for a live
        // context while its OSR variant still belongs to the dying one, or the
        // reverse.
        if proto.jit_osr_epoch.get() == epoch {
            proto.jit_osr_entry.set(None);
            proto.jit_osr_epoch.set(0);
            proto.jit_osr_ip.set(0);
            proto.jit_osr_code.replace(None);
            proto.jit_osr_failed.set(false);
            proto.backedge_count.set(0);
        }
        if proto.jit_epoch.get() != epoch {
            // Already recompiled for a live context; that owner clears it.
            continue;
        }
        proto.jit_entry.set(None);
        proto.clif_raw.set(0);
        proto.jit_code.replace(None);
        proto.jit_epoch.set(0);
        proto.jit_entry_count.set(0);
    }
}

/// Records `ctx` as the linking context for the duration of the guard,
/// restoring the previous one on drop (so nested runs compose).
pub struct CtxGuard(*const ExecCtx, u64);

impl CtxGuard {
    pub(crate) fn enter(ctx: *const ExecCtx) -> Self {
        let epoch = if ctx.is_null() {
            0
        } else {
            // Safety: same contract as `CtxLinker` — the caller keeps `ctx`
            // alive for the guard's lifetime.
            unsafe { (*ctx).heap.jit_epoch() }
        };
        let prev = CURRENT_CTX.with(|c| c.replace(ctx));
        let prev_epoch = CURRENT_EPOCH.with(|e| e.replace(epoch));
        CtxGuard(prev, prev_epoch)
    }
}

impl Drop for CtxGuard {
    fn drop(&mut self) {
        CURRENT_CTX.with(|c| c.set(self.0));
        CURRENT_EPOCH.with(|e| e.set(self.1));
    }
}

/// Linker bound to whatever context is current on this thread.
pub struct CtxLinker(*const ExecCtx);

impl CtxLinker {
    pub(crate) fn current() -> Self {
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
