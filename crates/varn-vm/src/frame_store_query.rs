use super::frame_store::{FrameStore, SlotAddr};
use varn_types::register_meta::SlotClass;

impl FrameStore {
    
    #[inline(always)]
    pub fn reg_class(&self, id: usize, reg: usize) -> SlotClass {
        self.allocs[id].layout.class_of(reg)
    }

    
    
    #[inline(always)]
    pub fn int_pair(&self, id: usize, r1: usize, r2: usize) -> Option<(i64, i64)> {
        let a = &self.allocs[id];
        let (c1, i1) = a.layout.slots[r1];
        let (c2, i2) = a.layout.slots[r2];
        if c1 == SlotClass::Gpr && c2 == SlotClass::Gpr {
            let b = a.bases[SlotClass::Gpr.index()] as usize;
            Some((self.gpr[b + i1 as usize], self.gpr[b + i2 as usize]))
        } else {
            None
        }
    }

    
    #[inline(always)]
    pub fn float_pair(&self, id: usize, r1: usize, r2: usize) -> Option<(f64, f64)> {
        let a = &self.allocs[id];
        let (c1, i1) = a.layout.slots[r1];
        let (c2, i2) = a.layout.slots[r2];
        if c1 == SlotClass::Fpr && c2 == SlotClass::Fpr {
            let b = a.bases[SlotClass::Fpr.index()] as usize;
            Some((self.fpr[b + i1 as usize], self.fpr[b + i2 as usize]))
        } else {
            None
        }
    }

    
    #[inline(always)]
    pub fn frame_count(&self) -> usize {
        self.allocs.len()
    }

    
    #[inline(always)]
    pub fn alloc_bases(&self, id: usize) -> [u32; 4] {
        self.allocs[id].bases
    }

    
    
    pub fn reg_of_addr(&self, id: usize, addr: SlotAddr) -> Option<usize> {
        let layout = &self.allocs[id].layout;
        let base = self.allocs[id].bases[addr.class.index()] as usize;
        if (addr.idx as usize) < base {
            return None;
        }
        let idx = addr.idx as usize - base;
        layout
            .slots
            .iter()
            .position(|(c, i)| *c == addr.class && *i as usize == idx)
    }
}
