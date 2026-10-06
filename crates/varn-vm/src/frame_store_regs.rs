use super::frame_store::{FrameStore, SlotAddr};
use super::frame_store_convert::value_kind_name;
use crate::error::{RuntimeError, VmResult};
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
        let i = a.idx as usize;
        match a.class {
            SlotClass::Gpr => VmValue::from_int(self.gpr[i]),
            SlotClass::Fpr => VmValue::from_f64(self.fpr[i]),
            SlotClass::Ref => self.refs[i].map_or(VmValue::null(), VmValue::from_heap),
            SlotClass::Dyn => self.dyn_[i],
        }
    }

    #[inline(always)]
    pub fn set_addr(&mut self, a: SlotAddr, v: VmValue) -> VmResult<()> {
        let i = a.idx as usize;
        match a.class {
            SlotClass::Gpr => {
                if !v.is_int() {
                    return Err(RuntimeError::new(format!(
                        "type mismatch: cannot store '{}' in an int register",
                        value_kind_name(v)
                    )));
                }
                self.gpr[i] = v.as_int();
            }
            SlotClass::Fpr => {
                
                
                
                
                
                
                
                
                
                
                
                if v.is_f64() {
                    self.fpr[i] = v.as_f64();
                } else if v.is_int() {
                    self.fpr[i] = v.as_int() as f64;
                } else if v.is_null() {
                    self.fpr[i] = f64::NAN;
                } else {
                    return Err(RuntimeError::new(format!(
                        "type mismatch: cannot store '{}' in a float register",
                        value_kind_name(v)
                    )));
                }
            }
            SlotClass::Ref => {
                
                
                
                if v.is_null() {
                    self.refs[i] = None;
                } else if v.is_heap() {
                    self.refs[i] = Some(v.as_heap());
                } else {
                    return Err(RuntimeError::new(format!(
                        "type mismatch: cannot store '{}' in a reference register",
                        value_kind_name(v)
                    )));
                }
            }
            SlotClass::Dyn => self.dyn_[i] = v,
        }
        Ok(())
    }
}
