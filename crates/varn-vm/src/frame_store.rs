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
//! por proto y vive en el propio proto (`FunctionProto::frame_layout`): ningún
//! almacén de frames la duplica. Cada activación reserva su tramo en los 4 vectores
//! ([`FrameStore::push_frame`]) y lo libera al retornar ([`FrameStore::pop_frame`]).
//!
//! Todo movimiento entre clases pasa por [`FrameStore::mov`]: misma clase es
//! copia cruda; hacia DYN es boxeo; desde DYN es unbox chequeado (un valor que
//! el checker probó estático siempre trae su tag; si no, es `type mismatch`,
//! no basura reinterpretada).

use std::rc::Rc;

use varn_types::FunctionProto;

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
}

impl FrameStore {
    pub fn new() -> Self {
        Self {
            gpr: Vec::with_capacity(4096),
            fpr: Vec::with_capacity(1024),
            refs: Vec::with_capacity(2048),
            dyn_: Vec::with_capacity(8192),
            allocs: Vec::with_capacity(512),
        }
    }

    pub fn new_for_task() -> Self {
        Self {
            gpr: Vec::new(),
            fpr: Vec::new(),
            refs: Vec::new(),
            dyn_: Vec::new(),
            allocs: Vec::new(),
        }
    }

    /// Reserva una activación y devuelve su id (el nuevo `base`).
    pub fn push_frame(&mut self, proto: &Rc<FunctionProto>) -> usize {
        let layout = proto.frame_layout();
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
    pub(crate) fn slot(&self, id: usize, reg: usize) -> (SlotClass, usize) {
        let a = &self.allocs[id];
        let (class, idx) = a.layout.slots[reg];
        (class, a.bases[class.index()] as usize + idx as usize)
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
