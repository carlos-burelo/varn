use super::frame_store::{FrameStore, SlotAddr};
use crate::error::VmResult;
use crate::value::VmValue;
use varn_types::register_meta::SlotClass;

impl FrameStore {
    #[inline(always)]
    pub fn g(&self, id: usize, reg: usize) -> i64 {
        let (class, i) = self.slot(id, reg);
        debug_assert_eq!(class, SlotClass::Gpr);
        self.gpr[i]
    }

    #[inline(always)]
    pub fn set_g(&mut self, id: usize, reg: usize, v: i64) {
        let (class, i) = self.slot(id, reg);
        debug_assert_eq!(class, SlotClass::Gpr);
        self.gpr[i] = v;
    }

    #[inline(always)]
    pub fn f(&self, id: usize, reg: usize) -> f64 {
        let (class, i) = self.slot(id, reg);
        debug_assert_eq!(class, SlotClass::Fpr);
        self.fpr[i]
    }

    #[inline(always)]
    pub fn set_f(&mut self, id: usize, reg: usize, v: f64) {
        let (class, i) = self.slot(id, reg);
        debug_assert_eq!(class, SlotClass::Fpr);
        self.fpr[i] = v;
    }

    #[inline(always)]
    pub fn d(&self, id: usize, reg: usize) -> VmValue {
        let (class, i) = self.slot(id, reg);
        debug_assert_eq!(class, SlotClass::Dyn);
        self.dyn_[i]
    }

    #[inline(always)]
    pub fn set_d(&mut self, id: usize, reg: usize, v: VmValue) {
        let (class, i) = self.slot(id, reg);
        debug_assert_eq!(class, SlotClass::Dyn);
        self.dyn_[i] = v;
    }

    #[inline(always)]
    pub fn addr_of(&self, id: usize, reg: usize) -> SlotAddr {
        let (class, i) = self.slot(id, reg);
        SlotAddr {
            class,
            idx: i as u32,
        }
    }

    #[inline(always)]
    pub fn get_addr(&self, a: SlotAddr) -> VmValue {
        self.box_slot(a.class, a.idx as usize)
    }

    #[inline(always)]
    pub fn set_addr(&mut self, a: SlotAddr, v: VmValue) -> VmResult<()> {
        self.unbox_into(a.class, a.idx as usize, v)
    }
}
