use super::frame_store::{FrameStore, REF_UNINIT};
use crate::error::{RuntimeError, VmResult};
use crate::value::VmValue;
use varn_types::register_meta::SlotClass;

/// Nombre de la clase de un valor boxeado, para errores de conversión.
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
    // ── Movimiento entre registros (el `Move` del bytecode) ──────────

    /// Mueve `src` a `dst` dentro de la misma activación, convirtiendo entre
    /// clases: misma clase copia cruda; hacia DYN boxea; desde DYN chequea el
    /// tag (nunca reinterpreta).
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

    /// Mueve entre activaciones (retornos, throws, staging de llamadas).
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
                // Ensanchado `int` → `float` en llamadas (`coherence` lo
                // permite) y la misma coerción perezosa que el intérprete
                // aplicaba en cada uso (`as_int as f64`).
                //
                // Simétrico con `box_slot`: `VmValue::from_f64` ya convierte
                // NaN a `null` (`vm_value.rs`), así que un `float` cuyo
                // resultado fue NaN (`inf * 0.0`, `x - x` con `x` infinito)
                // llega aquí como `null`, no como el NaN crudo — guardar NaN
                // de vuelta deja que `box_slot` lo recupere como `null`.
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
                // Simétrico con `box_slot`: `null` es `REF_UNINIT`, no un
                // error — un retorno `void` o una referencia nula son el
                // valor, no basura a rechazar.
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

    /// Boxea un registro a `VmValue` (fronteras: heap, nativas, suspensión).
    #[inline(always)]
    pub fn box_reg(&self, id: usize, reg: usize) -> VmValue {
        let (class, i) = self.slot(id, reg);
        self.box_slot(class, i)
    }

    /// Escribe un `VmValue` convirtiendo a la clase del registro.
    #[inline(always)]
    pub fn unbox_into_reg(&mut self, id: usize, reg: usize, v: VmValue) -> VmResult<()> {
        let (class, i) = self.slot(id, reg);
        self.unbox_into(class, i, v)
    }

    /// Boxea un rango de registros (ventanas de llamadas nativas).
    pub fn box_range(&self, id: usize, start: usize, count: usize) -> Vec<VmValue> {
        (0..count).map(|k| self.box_reg(id, start + k)).collect()
    }

    /// Adopta valores boxeados en registros (retornos host→VM, generadores).
    /// Rellena con defaults de clase si faltan; ignora sobrantes duplicados.
    pub fn adopt_values(&mut self, id: usize, start: usize, vals: &[VmValue], nregs: usize) {
        for r in start..start + nregs {
            let v = vals.get(r - start).copied().unwrap_or_else(VmValue::null);
            let (class, i) = self.slot(id, r);
            match class {
                SlotClass::Gpr => self.gpr[i] = if v.is_int() { v.as_int() } else { 0 },
                SlotClass::Fpr => self.fpr[i] = if v.is_f64() { v.as_f64() } else { 0.0 },
                SlotClass::Ref => {
                    self.refs[i] = if v.is_heap() {
                        v.as_heap_idx()
                    } else {
                        REF_UNINIT
                    }
                }
                SlotClass::Dyn => self.dyn_[i] = v,
            }
        }
    }
}
