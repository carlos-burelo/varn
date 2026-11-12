use std::rc::Rc;
use std::sync::Arc;

use super::super::inline_cache::{FeedbackVector, PolyICSlot};
use super::super::pool::PoolEntry;
use super::definition::FunctionProto;

impl FunctionProto {
    pub fn osr_lookup(
        &self,
        ip: usize,
        mode: crate::ssa::OsrMode,
        epoch: u64,
    ) -> Option<Option<usize>> {
        self.jit_osr_entries
            .borrow()
            .iter()
            .find_map(|e| (e.ip == ip && e.mode == mode && e.epoch == epoch).then_some(e.entry))
    }

    pub fn osr_failed(&self, ip: usize, mode: crate::ssa::OsrMode, epoch: u64) -> bool {
        matches!(self.osr_lookup(ip, mode, epoch), Some(None))
    }

    pub fn osr_remember(
        &self,
        ip: usize,
        mode: crate::ssa::OsrMode,
        epoch: u64,
        entry: Option<(usize, Rc<dyn std::any::Any>)>,
    ) {
        let mut entries = self.jit_osr_entries.borrow_mut();
        entries.retain(|e| !(e.ip == ip && e.mode == mode));
        let (ptr, code) = match entry {
            Some((ptr, code)) => (Some(ptr), Some(code)),
            None => (None, None),
        };
        entries.push(super::definition::OsrCacheEntry {
            ip,
            mode,
            epoch,
            entry: ptr,
            code,
        });
    }

    pub fn osr_invalidate(&self, epoch: u64) {
        self.jit_osr_entries
            .borrow_mut()
            .retain(|e| e.epoch != epoch);
    }

    pub fn frame_layout(&self) -> Rc<crate::register_meta::FrameLayout> {
        Rc::clone(
            self.frame_layout
                .get_or_init(|| Rc::new(crate::register_meta::FrameLayout::for_proto(self))),
        )
    }

    pub fn ensure_ic(&self) {
        let n = self.cache_count;
        if n == 0 {
            return;
        }
        let mut ic = self.ic_cache.borrow_mut();
        if ic.is_empty() {
            ic.resize_with(n, PolyICSlot::new);
        }
        let mut fb = self.feedback.borrow_mut();
        if fb.sites.is_empty() {
            *fb = FeedbackVector::new(n);
        }
    }

    pub fn resolved_shape(&self, idx: usize) -> Option<Rc<crate::Shape>> {
        if let Some((_, s)) = self
            .resolved_shapes
            .borrow()
            .iter()
            .find(|(i, _)| *i as usize == idx)
        {
            return Some(Rc::clone(s));
        }
        let keys = match self.chunk.constants.get(idx) {
            Some(PoolEntry::Shape(k)) => k,
            Some(
                PoolEntry::Literal(_)
                | PoolEntry::Function(_)
                | PoolEntry::Layout(_)
                | PoolEntry::Variant(_),
            )
            | None => return None,
        };
        let mut shape = crate::root_shape();
        for k in keys {
            shape = shape.transition(Arc::clone(k));
        }
        self.resolved_shapes
            .borrow_mut()
            .push((idx as u32, Rc::clone(&shape)));
        Some(shape)
    }
}
