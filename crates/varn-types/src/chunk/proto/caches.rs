use std::rc::Rc;
use std::sync::Arc;

use super::super::inline_cache::{FeedbackVector, PolyICSlot};
use super::super::pool::PoolEntry;
use super::definition::FunctionProto;

impl FunctionProto {
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
            Some(PoolEntry::Literal(_) | PoolEntry::Function(_) | PoolEntry::Layout(_)) | None => return None,
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
