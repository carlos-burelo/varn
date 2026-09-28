use super::frame_store::{FrameStore, SlotAddr, REF_UNINIT};
use super::frame_store_convert::value_kind_name;
use crate::error::{RuntimeError, VmResult};
use crate::value::VmValue;
use varn_types::register_meta::SlotClass;

impl FrameStore {
    // ── Accesores por clase ──────────────────────────────────────────
    // Cada uno afirma en debug la clase del slot: un acceso con clase
    // equivocada es un bug del emisor, nunca un dato a reinterpretar.

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
    pub fn r(&self, id: usize, reg: usize) -> u32 {
        let (class, i) = self.slot(id, reg);
        debug_assert_eq!(class, SlotClass::Ref);
        self.refs[i]
    }

    #[inline(always)]
    pub fn set_r(&mut self, id: usize, reg: usize, v: u32) {
        let (class, i) = self.slot(id, reg);
        debug_assert_eq!(class, SlotClass::Ref);
        self.refs[i] = v;
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

    /// Dirección estable del slot (para upvalues abiertos).
    #[inline(always)]
    pub fn addr_of(&self, id: usize, reg: usize) -> SlotAddr {
        let (class, i) = self.slot(id, reg);
        SlotAddr {
            class,
            idx: i as u32,
        }
    }

    // ── Lectura/escritura por dirección (upvalues, GC, debug) ────────

    #[inline(always)]
    pub fn get_addr(&self, a: SlotAddr) -> VmValue {
        let i = a.idx as usize;
        match a.class {
            SlotClass::Gpr => VmValue::from_int(self.gpr[i]),
            SlotClass::Fpr => VmValue::from_f64(self.fpr[i]),
            SlotClass::Ref => {
                let h = self.refs[i];
                if h == REF_UNINIT {
                    VmValue::null()
                } else {
                    VmValue::from_heap_idx(h)
                }
            }
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
                // Ensanchado `int` → `float` en llamadas (`coherence` lo
                // permite) y la misma coerción perezosa que el intérprete
                // aplicaba en cada uso (`as_int as f64`).
                //
                // `null` es representable aquí igual que en Ref, pero sin
                // sentinel aparte: `VmValue::from_f64` YA convierte NaN a
                // `null` (`vm_value.rs`), así que un `float` cuyo resultado
                // fue NaN (`inf * 0.0`, `x - x` con `x` infinito) llega a
                // este punto como `null`, no como el NaN crudo. Guardar un
                // NaN de vuelta es lo simétrico: `box_slot` lo vuelve a leer
                // como `null` a través del mismo `from_f64`.
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
                // `null` es representable en un registro Ref (`REF_UNINIT`,
                // simétrico con `get_addr`/`box_slot`): un retorno `void` o
                // una referencia nula no son "basura", son el valor.
                if v.is_null() {
                    self.refs[i] = REF_UNINIT;
                } else if v.is_heap() {
                    self.refs[i] = v.as_heap_idx();
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
