use super::*;

impl ObjData<[Cell<VmValue>]> {
    #[inline(always)]
    pub fn shape(&self) -> &Rc<Shape> {
        unsafe { &*self.shape.get() }
    }

    #[inline]
    pub fn set_shape(&self, shape: Rc<Shape>) {
        unsafe { *self.shape.get() = shape }
    }

    #[inline(always)]
    pub fn inline_len(&self) -> usize {
        self.inline_len as usize
    }

    #[inline(always)]
    pub fn inline_slice(&self) -> &[Cell<VmValue>] {
        &self.values
    }

    #[inline(always)]
    pub(super) fn overflow(&self) -> Option<&Vec<VmValue>> {
        unsafe { (*self.overflow.get()).as_deref() }
    }

    #[allow(clippy::mut_from_ref)]
    #[inline]
    pub(super) fn overflow_mut(&self) -> &mut Vec<VmValue> {
        unsafe { (*self.overflow.get()).get_or_insert_with(Box::default) }
    }

    #[inline]
    pub fn slot_count(&self) -> usize {
        self.inline_len() + self.overflow().map_or(0, |o| o.len())
    }

    #[inline(always)]
    pub fn field_at(&self, slot: usize) -> Option<VmValue> {
        match self.values.get(slot) {
            Some(c) => Some(c.get()),
            None => self
                .overflow()
                .and_then(|o| o.get(slot - self.inline_len()))
                .copied(),
        }
    }

    #[inline(always)]
    pub fn set_field_at(&self, slot: usize, value: VmValue) -> bool {
        if let Some(c) = self.values.get(slot) {
            c.set(value);
            return true;
        }
        let off = slot - self.inline_len();
        let overflow = self.overflow_mut();
        match overflow.get_mut(off) {
            Some(s) => {
                *s = value;
                true
            }
            None => false,
        }
    }

    #[inline]
    pub fn for_each_field(&self, mut f: impl FnMut(usize, VmValue)) {
        for (i, c) in self.values.iter().enumerate() {
            f(i, c.get());
        }
        if let Some(o) = self.overflow() {
            let base = self.inline_len();
            for (i, v) in o.iter().enumerate() {
                f(base + i, *v);
            }
        }
    }
}
