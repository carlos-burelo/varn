use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use rustc_hash::FxHashMap;
use varn_jit::clif::lower::ClifLinker;
use varn_types::FunctionProto;

use crate::exec::ExecCtx;

thread_local! {
    static CURRENT_CTX: Cell<*const ExecCtx> = const { Cell::new(std::ptr::null()) };


    static CURRENT_EPOCH: Cell<u64> = const { Cell::new(0) };


    static COMPILED: RefCell<FxHashMap<u64, EpochCode>> =
        RefCell::new(FxHashMap::default());
}

#[derive(Default)]
struct EpochCode {
    protos: Vec<Rc<FunctionProto>>,

    retired: Vec<Rc<dyn std::any::Any>>,
}

static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_epoch() -> u64 {
    NEXT_EPOCH.fetch_add(1, Ordering::Relaxed)
}

#[inline(always)]
pub(crate) fn current_epoch() -> u64 {
    CURRENT_EPOCH.with(|e| e.get())
}

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

pub(crate) fn retire_code(epoch: u64, code: Rc<dyn std::any::Any>) {
    COMPILED.with(|m| m.borrow_mut().entry(epoch).or_default().retired.push(code));
}

pub(crate) fn invalidate_epoch(epoch: u64) {
    let entry = COMPILED.with(|m| m.borrow_mut().remove(&epoch));
    let Some(entry) = entry else { return };
    for proto in &entry.protos {
        if proto.jit_osr_epoch.get() == epoch {
            proto.jit_osr_entry.set(None);
            proto.jit_osr_epoch.set(0);
            proto.jit_osr_ip.set(0);
            proto.jit_osr_code.replace(None);
            proto.jit_osr_failed.set(false);
            proto.backedge_count.set(0);
        }
        if proto.jit_epoch.get() != epoch {
            continue;
        }
        proto.jit_entry.set(0);
        proto.jit_native.set(0);
        proto.jit_native_sig.set(0);
        proto.jit_code.replace(None);
        proto.jit_epoch.set(0);
        proto.jit_entry_count.set(0);
    }
}

pub struct CtxGuard(*const ExecCtx, u64);

impl CtxGuard {
    pub(crate) fn enter(ctx: *const ExecCtx) -> Self {
        let epoch = if ctx.is_null() {
            0
        } else {
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

pub struct CtxLinker;

impl ClifLinker for CtxLinker {
    fn current_epoch(&self) -> u64 {
        current_epoch()
    }
}
