use super::frame_store::FrameStore;
use crate::error::{RuntimeError, VmResult};
use crate::value::VmValue;
use varn_types::register_meta::SlotClass;

pub(crate) fn value_kind_name(v: VmValue) -> &'static str {
    use varn_types::vm_value::*;
    match v.kind() {
        KIND_NULL => "null",
        KIND_BOOL => "bool",
        KIND_INT => "int",
        KIND_FLOAT => "float",
        KIND_HEAP => "reference",
        KIND_SSO => "str",
        KIND_SYMBOL => "symbol",
        _ => "dynamic",
    }
}

impl FrameStore {
    #[inline(always)]
    pub fn mov(&mut self, id: usize, dst: usize, src: usize) -> VmResult<()> {
        let (sc, si) = self.slot(id, src);
        let (dc, di) = self.slot(id, dst);
        if sc == dc {
            match sc {
                SlotClass::Gpr => self.gpr[di] = self.gpr[si],
                SlotClass::Fpr => self.fpr[di] = self.fpr[si],
                SlotClass::Ref => self.refs[di] = self.refs[si],
                SlotClass::Dyn => self.dyn_[di] = self.dyn_[si],
            }
            return Ok(());
        }
        let v = self.box_slot(sc, si);
        self.unbox_into(dc, di, v)
    }

    #[inline(always)]
    pub fn mov_cross(
        &mut self,
        dst_id: usize,
        dst: usize,
        src_id: usize,
        src: usize,
    ) -> VmResult<()> {
        let (sc, si) = self.slot(src_id, src);
        let (dc, di) = self.slot(dst_id, dst);
        if sc == dc {
            match sc {
                SlotClass::Gpr => {
                    let v = self.gpr[si];
                    self.gpr[di] = v;
                }
                SlotClass::Fpr => {
                    let v = self.fpr[si];
                    self.fpr[di] = v;
                }
                SlotClass::Ref => {
                    let v = self.refs[si];
                    self.refs[di] = v;
                }
                SlotClass::Dyn => {
                    let v = self.dyn_[si];
                    self.dyn_[di] = v;
                }
            }
            return Ok(());
        }
        let v = self.box_slot(sc, si);
        self.unbox_into(dc, di, v)
    }

    #[inline(always)]
    fn box_slot(&self, class: SlotClass, i: usize) -> VmValue {
        match class {
            SlotClass::Gpr => VmValue::from_int(self.gpr[i]),
            SlotClass::Fpr => VmValue::from_f64(self.fpr[i]),
            SlotClass::Ref => self.refs[i].map_or(VmValue::null(), VmValue::from_heap),
            SlotClass::Dyn => self.dyn_[i],
        }
    }

    #[inline(always)]
    fn unbox_into(&mut self, class: SlotClass, i: usize, v: VmValue) -> VmResult<()> {
        match class {
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

    #[inline(always)]
    pub fn box_reg(&self, id: usize, reg: usize) -> VmValue {
        let (class, i) = self.slot(id, reg);
        self.box_slot(class, i)
    }

    #[inline(always)]
    pub fn unbox_into_reg(&mut self, id: usize, reg: usize, v: VmValue) -> VmResult<()> {
        let (class, i) = self.slot(id, reg);
        self.unbox_into(class, i, v)
    }

    pub fn box_range(&self, id: usize, start: usize, count: usize) -> Vec<VmValue> {
        (0..count).map(|k| self.box_reg(id, start + k)).collect()
    }

    pub fn adopt_values(&mut self, id: usize, start: usize, vals: &[VmValue], nregs: usize) {
        for r in start..start + nregs {
            let v = vals.get(r - start).copied().unwrap_or_else(VmValue::null);
            let (class, i) = self.slot(id, r);
            match class {
                SlotClass::Gpr => self.gpr[i] = if v.is_int() { v.as_int() } else { 0 },
                SlotClass::Fpr => self.fpr[i] = if v.is_f64() { v.as_f64() } else { 0.0 },
                SlotClass::Ref => self.refs[i] = v.is_heap().then(|| v.as_heap()),
                SlotClass::Dyn => self.dyn_[i] = v,
            }
        }
    }
}
