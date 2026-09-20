//! Frame por clases: el registro deja de ser un `VmValue` universal.
//!
//! Cada registro vive en el almacén de su clase, derivada de `register_meta`
//! (la prueba del checker que el backend ya serializa por función):
//!
//! ```text
//! GPR (i64)  ← SlotKind::Int
//! FPR (f64)  ← SlotKind::Float
//! REF (u32)  ← SlotKind::Ref      (índice heap; nunca null)
//! DYN        ← SlotKind::{Dynamic, Bool, Str} (VmValue, como antes)
//! ```
//!
//! `Bool` y `Str` se quedan en DYN a propósito: hoy no existe desempaquetado
//! para ellos en ningún camino (comparaciones, `JumpIfFalse`, SSO/heap-str),
//! y moverlos a GPR cambiaría la semántica de truthiness. Son el siguiente
//! paso, no este.
//!
//! La traducción registro → (clase, índice) la calcula [`FrameLayout`] una vez
//! por proto (cacheado por dirección del `Rc`, reteniendo el `Rc` como hacen
//! `proto_constants`: sin el `Rc` la dirección sería reutilizable tras `drop`
//! y la caché mentiría). Cada activación reserva su tramo en los 4 vectores
//! ([`FrameStore::push_frame`]) y lo libera al retornar ([`FrameStore::pop_frame`]).
//!
//! Todo movimiento entre clases pasa por [`FrameStore::mov`]: misma clase es
//! copia cruda; hacia DYN es boxeo; desde DYN es unbox chequeado (un valor que
//! el checker probó estático siempre trae su tag; si no, es `type mismatch`,
//! no basura reinterpretada).

use rustc_hash::FxHashMap as HashMap;
use std::rc::Rc;

use varn_types::FunctionProto;

use crate::error::{RuntimeError, VmResult};
use crate::value::VmValue;

/// La clase física vive en `varn-types` (contrato único VM+JIT); aquí solo se
/// re-exporta para que los call-sites de la VM la sigan nombrando por este
/// módulo. Ver `varn_types::register_meta::SlotClass` para la proyección.
pub use varn_types::register_meta::SlotClass;

/// Dirección estable de un slot dentro del almacén.
///
/// A diferencia del antiguo índice absoluto en un único `Vec`, no caduca al
/// crecer otros vectores: cada vector solo crece por el final y solo se
/// trunca la región de frames ya retornados (cuyos upvalues se cerraron
/// antes, por disciplina existente).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SlotAddr {
    pub class: SlotClass,
    pub idx: u32,
}

/// El layout registro→(clase, índice) y el sentinel `REF_UNINIT` viven en
/// `varn-types` (contrato compartido VM+JIT); aquí solo se re-exportan para que
/// los call-sites de la VM los sigan nombrando por este módulo. Ver
/// `varn_types::register_meta::{FrameLayout, REF_UNINIT}`.
pub use varn_types::register_meta::{FrameLayout, REF_UNINIT};

/// Reserva de una activación: bases por clase dentro del almacén.
///
/// `#[repr(C)]`: `bases` va primero y a offset fijo, porque el ABI del JIT
/// direcciona `allocs[act_id].bases[clase]` desde código generado. Ver el
/// contrato de fase B en `docs/plans/2026-09-20-PLAN-PENDIENTE.md`.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct FrameAlloc {
    pub bases: [u32; 4],
    pub layout: Rc<FrameLayout>,
}

/// Nombre de la clase de un valor boxeado, para errores de conversión.
fn value_kind_name(v: VmValue) -> &'static str {
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

/// Pila de activaciones partida por clases.
///
/// `#[repr(C)]`: los cuatro vectores por clase van primero y en orden, porque
/// el ABI del JIT carga los punteros de datos de cada clase desde código
/// generado (ver `JitFrameLayout`). El orden de campos es parte del contrato.
#[repr(C)]
#[derive(Debug, Default)]
pub struct FrameStore {
    pub gpr: Vec<i64>,
    pub fpr: Vec<f64>,
    pub refs: Vec<u32>,
    pub dyn_: Vec<VmValue>,
    /// Reservas de activación, indexadas por el `act_id` que ve el JIT. Parte
    /// del ABI (el lowering lee `allocs[act_id].bases[clase]`), por eso es
    /// `pub(crate)` en vez de privado.
    pub(crate) allocs: Vec<FrameAlloc>,
    layouts: HashMap<usize, (Rc<FunctionProto>, Rc<FrameLayout>)>,
}

impl FrameStore {
    pub fn new() -> Self {
        Self {
            gpr: Vec::with_capacity(4096),
            fpr: Vec::with_capacity(1024),
            refs: Vec::with_capacity(2048),
            dyn_: Vec::with_capacity(8192),
            allocs: Vec::with_capacity(512),
            layouts: HashMap::default(),
        }
    }

    /// Layout cacheado por proto (retiene el `Rc`: ver docs del módulo).
    pub fn layout_for(&mut self, proto: &Rc<FunctionProto>) -> Rc<FrameLayout> {
        let key = Rc::as_ptr(proto) as usize;
        if let Some((_, layout)) = self.layouts.get(&key) {
            return Rc::clone(layout);
        }
        let layout = Rc::new(FrameLayout::for_proto(proto));
        self.layouts
            .insert(key, (Rc::clone(proto), Rc::clone(&layout)));
        layout
    }

    /// Reserva una activación y devuelve su id (el nuevo `base`).
    pub fn push_frame(&mut self, proto: &Rc<FunctionProto>) -> usize {
        let layout = self.layout_for(proto);
        let id = self.allocs.len();
        let mut bases = [0u32; 4];
        bases[SlotClass::Gpr.index()] = self.gpr.len() as u32;
        bases[SlotClass::Fpr.index()] = self.fpr.len() as u32;
        bases[SlotClass::Ref.index()] = self.refs.len() as u32;
        bases[SlotClass::Dyn.index()] = self.dyn_.len() as u32;
        self.gpr
            .extend(std::iter::repeat(0).take(layout.counts[SlotClass::Gpr.index()] as usize));
        self.fpr
            .extend(std::iter::repeat(0.0).take(layout.counts[SlotClass::Fpr.index()] as usize));
        self.refs.extend(
            std::iter::repeat(REF_UNINIT).take(layout.counts[SlotClass::Ref.index()] as usize),
        );
        self.dyn_.extend(
            std::iter::repeat(VmValue::null()).take(layout.counts[SlotClass::Dyn.index()] as usize),
        );
        self.allocs.push(FrameAlloc { bases, layout });
        id
    }

    /// Libera la activación superior (disciplina LIFO, como antes).
    pub fn pop_frame(&mut self) {
        if let Some(alloc) = self.allocs.pop() {
            self.gpr
                .truncate(alloc.bases[SlotClass::Gpr.index()] as usize);
            self.fpr
                .truncate(alloc.bases[SlotClass::Fpr.index()] as usize);
            self.refs
                .truncate(alloc.bases[SlotClass::Ref.index()] as usize);
            self.dyn_
                .truncate(alloc.bases[SlotClass::Dyn.index()] as usize);
        }
    }

    /// Libera todo por encima de `id` (sin incluirlo) — unwind.
    pub fn pop_above(&mut self, id: usize) {
        while self.allocs.len() > id + 1 {
            self.pop_frame();
        }
    }

    /// Asegura que la activación `id` direcciona `register_count` registros
    /// (extiende con defaults si un trailing nunca se escribió).
    pub fn ensure_frame_size(&mut self, id: usize, register_count: usize) {
        let (bases, counts) = {
            let a = &self.allocs[id];
            (a.bases, a.layout.counts)
        };
        let need = |base: u32, count: u32, len: usize| base as usize + count as usize > len;
        if need(bases[0], counts[0], self.gpr.len()) {
            self.gpr.resize(bases[0] as usize + counts[0] as usize, 0);
        }
        if need(bases[1], counts[1], self.fpr.len()) {
            self.fpr.resize(bases[1] as usize + counts[1] as usize, 0.0);
        }
        if need(bases[2], counts[2], self.refs.len()) {
            self.refs
                .resize(bases[2] as usize + counts[2] as usize, REF_UNINIT);
        }
        if need(bases[3], counts[3], self.dyn_.len()) {
            self.dyn_
                .resize(bases[3] as usize + counts[3] as usize, VmValue::null());
        }
        let _ = register_count;
    }

    #[inline(always)]
    fn slot(&self, id: usize, reg: usize) -> (SlotClass, usize) {
        let a = &self.allocs[id];
        let (class, idx) = a.layout.slots[reg];
        (class, a.bases[class.index()] as usize + idx as usize)
    }

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

    // ── Raíces GC ────────────────────────────────────────────────────
    // GPR/FPR nunca son raíces por construcción: el colector ni los mira.

    /// Extremo vivo del tramo DYN (las activaciones son contiguas desde 0).
    #[inline(always)]
    pub fn dyn_live_top(&self) -> usize {
        self.dyn_.len()
    }

    /// Extremo vivo del tramo REF.
    #[inline(always)]
    pub fn ref_live_top(&self) -> usize {
        self.refs.len()
    }

    #[inline(always)]
    pub fn dyn_slice_mut(&mut self) -> &mut [VmValue] {
        &mut self.dyn_
    }

    #[inline(always)]
    pub fn ref_slice_mut(&mut self) -> &mut [u32] {
        &mut self.refs
    }

    /// Recoge raíces para major GC: refs directas + dyn filtradas.
    pub fn collect_roots(&self, top_dyn: usize, top_ref: usize, out: &mut Vec<u32>) {
        for &h in &self.refs[..top_ref.min(self.refs.len())] {
            if h != REF_UNINIT {
                out.push(h);
            }
        }
        for v in &self.dyn_[..top_dyn.min(self.dyn_.len())] {
            if v.is_heap() {
                out.push(v.as_heap_idx());
            }
        }
    }

    /// Clase del registro (para vías rápidas por clase en dispatch).
    #[inline(always)]
    pub fn reg_class(&self, id: usize, reg: usize) -> SlotClass {
        self.allocs[id].layout.class_of(reg)
    }

    /// Par int directo si ambos registros son GPR (por construcción solo
    /// alojan ints: sin chequeo de tag). `None` → camino boxeado.
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

    /// Par float directo si ambos registros son FPR. `None` → boxeado.
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

    /// Nº de activaciones vivas.
    #[inline(always)]
    pub fn frame_count(&self) -> usize {
        self.allocs.len()
    }

    /// Bases por clase de la activación `id` (para cierres de upvalues).
    #[inline(always)]
    pub fn alloc_bases(&self, id: usize) -> [u32; 4] {
        self.allocs[id].bases
    }

    /// Registro que ocupa `addr` dentro de la activación `id`, si es suyo
    /// (para `CloseUpvalue` parcial por registro).
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

#[cfg(test)]
mod tests {
    use super::*;
    use varn_types::register_meta::{RegisterMeta, SlotKind};

    fn proto_with(meta: &[SlotKind], nregs: u16) -> Rc<FunctionProto> {
        let mut p = FunctionProto::default();
        p.register_count = nregs;
        p.register_meta = meta.iter().map(|&kind| RegisterMeta { kind }).collect();
        Rc::new(p)
    }

    #[test]
    fn layout_partitions_by_class() {
        use SlotKind as K;
        let p = proto_with(&[K::Dynamic, K::Int, K::Float, K::Int, K::Ref], 5);
        let l = FrameLayout::for_proto(&p);
        assert_eq!(l.counts, [2, 1, 1, 1]);
        assert_eq!(l.slots[0], (SlotClass::Dyn, 0));
        assert_eq!(l.slots[1], (SlotClass::Gpr, 0));
        assert_eq!(l.slots[2], (SlotClass::Fpr, 0));
        assert_eq!(l.slots[3], (SlotClass::Gpr, 1));
        assert_eq!(l.slots[4], (SlotClass::Ref, 0));
    }

    #[test]
    fn bool_and_str_stay_dynamic() {
        use SlotKind as K;
        let p = proto_with(&[K::Bool, K::Str], 2);
        let l = FrameLayout::for_proto(&p);
        assert_eq!(l.counts, [0, 0, 0, 2]);
    }

    #[test]
    fn push_pop_isolates_frames() {
        use SlotKind as K;
        let mut s = FrameStore::new();
        let p = proto_with(&[K::Int, K::Float], 2);
        let a = s.push_frame(&p);
        s.set_g(a, 0, 7);
        s.set_f(a, 1, 1.5);
        let b = s.push_frame(&p);
        assert_eq!(s.g(b, 0), 0);
        assert_eq!(s.f(b, 1), 0.0);
        s.set_g(b, 0, 9);
        assert_eq!(s.g(a, 0), 7);
        s.pop_frame();
        assert_eq!(s.g(a, 0), 7);
        assert_eq!(s.frame_count(), 1);
    }

    #[test]
    fn mov_converts_between_classes() {
        use SlotKind as K;
        let mut s = FrameStore::new();
        // r0=Dyn, r1=Int, r2=Float, r3=Ref
        let p = proto_with(&[K::Dynamic, K::Int, K::Float, K::Ref], 4);
        let a = s.push_frame(&p);
        s.set_g(a, 1, 42);
        s.mov(a, 0, 1).unwrap();
        assert_eq!(s.d(a, 0), VmValue::from_int(42));
        // Dyn(int) -> Int: chequeado, pasa.
        s.mov(a, 1, 0).unwrap();
        assert_eq!(s.g(a, 1), 42);
        // Dyn(bool) -> Int: error de tipos, no basura.
        s.set_d(a, 0, VmValue::from_bool(true));
        assert!(s.mov(a, 1, 0).is_err());
        assert_eq!(s.g(a, 1), 42);
        // Float <-> Dyn ida y vuelta exacta.
        s.set_f(a, 2, 2.5);
        s.mov(a, 0, 2).unwrap();
        assert_eq!(s.d(a, 0), VmValue::from_f64(2.5));
        s.mov(a, 2, 0).unwrap();
        assert_eq!(s.f(a, 2), 2.5);
    }

    #[test]
    fn ref_slots_roundtrip_heap_idx() {
        use SlotKind as K;
        let mut s = FrameStore::new();
        let p = proto_with(&[K::Ref], 1);
        let a = s.push_frame(&p);
        s.set_r(a, 0, 123);
        assert_eq!(s.r(a, 0), 123);
        assert_eq!(s.box_reg(a, 0), VmValue::from_heap_idx(123));
        let mut roots = Vec::new();
        s.collect_roots(0, 1, &mut roots);
        assert_eq!(roots, vec![123]);
    }

    #[test]
    fn uninit_ref_is_skipped_by_roots_and_reads_null() {
        use SlotKind as K;
        let mut s = FrameStore::new();
        let p = proto_with(&[K::Ref], 1);
        let a = s.push_frame(&p);
        let addr = s.addr_of(a, 0);
        assert_eq!(s.get_addr(addr), VmValue::null());
        let mut roots = Vec::new();
        s.collect_roots(0, 1, &mut roots);
        assert!(roots.is_empty());
    }
}
