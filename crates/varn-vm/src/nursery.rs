use crate::heap::{HeapInner, HeapObj};
use crate::value::VmValue;
use std::rc::Rc;

pub const OLD_GEN_FLAG: u32 = 0x8000_0000;
pub const NURSERY_CAPACITY: usize = 65536;

#[inline(always)]
pub(crate) fn is_nursery_idx(idx: u32) -> bool {
    (idx & OLD_GEN_FLAG) == 0
}

#[inline(always)]
pub(crate) fn is_old_idx(idx: u32) -> bool {
    (idx & OLD_GEN_FLAG) != 0
}

#[inline(always)]
pub(crate) fn old_idx_raw(packed: u32) -> u32 {
    packed & !OLD_GEN_FLAG
}

#[inline(always)]
pub(crate) fn pack_old_idx(raw_old: u32) -> u32 {
    raw_old | OLD_GEN_FLAG
}

pub struct Nursery {
    objects: Vec<Option<HeapObj>>,
    forwarding: Vec<Option<u32>>,
    pub remembered: Vec<u32>,
    /// Buffers de trabajo del colector menor, propiedad del `Nursery` para
    /// conservar su capacidad entre colecciones. Antes eran locales de
    /// `collect`, así que cada minor GC asignaba y liberaba 256 KB de
    /// worklist más una copia del vector de raíces del old gen; ese coste era
    /// fijo por colección, no proporcional a lo que sobrevive.
    ///
    /// `collect` los saca con `mem::take` y los devuelve al terminar: los
    /// métodos que los consumen también toman `&mut self`, así que no pueden
    /// prestarse como campos a la vez.
    worklist: Vec<u32>,
    scan_candidates: Vec<u32>,
    /// SONDA: fase de `collect` en curso, para que el aviso de referencia
    /// colgante diga de qué conjunto de raíces salió.
    phase: &'static str,
    pub alloc_count: u64,
    pub minor_gc_count: u64,
    pub minor_gc_promoted: u64,
}

impl Default for Nursery {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Nursery {
    /// Hand-written, not derived. A derived `Clone` would clone `objects`
    /// and `forwarding` via `Vec::clone` (`slice::to_vec`), which allocates
    /// with `capacity == len` — silently dropping the "capacity is
    /// `NURSERY_CAPACITY` from birth and never changes" invariant `new`
    /// establishes and `emit_nursery_alloc` depends on for its raw slot
    /// address to stay valid.
    ///
    /// This is reachable, not theoretical: `Heap::deep_clone` clones
    /// `HeapInner`, which clones `Nursery`; `deep_clone`'s only caller is
    /// `Vm::from_snapshot`, whose only caller is the bench harness
    /// (`crates/varn-cli/src/bench/harness.rs`) — every single `vn bench`
    /// iteration deep-clones a fresh heap this way. Without this override,
    /// every VM `vn bench` builds would run on a nursery whose backing
    /// store can move at the first `push` that exceeds its (small,
    /// length-sized) cloned capacity — silent corruption the moment a
    /// caller (Task 7) holds a raw slot address across such a `push`.
    ///
    /// Reserves exactly like `new`, then copies contents in — same
    /// allocation cost `new` already pays, paid again here rather than
    /// left cheaper-but-unsound.
    fn clone(&self) -> Self {
        let mut objects = Vec::with_capacity(NURSERY_CAPACITY);
        objects.extend(self.objects.iter().cloned());
        let mut forwarding = Vec::with_capacity(NURSERY_CAPACITY);
        forwarding.extend(self.forwarding.iter().cloned());
        Self {
            objects,
            forwarding,
            remembered: self.remembered.clone(),
            worklist: Vec::new(),
            phase: "?",
            scan_candidates: Vec::new(),
            alloc_count: self.alloc_count,
            minor_gc_count: self.minor_gc_count,
            minor_gc_promoted: self.minor_gc_promoted,
        }
    }
}

impl Nursery {
    pub(crate) fn new() -> Self {
        // Full capacity from birth, not grown into. `try_alloc` pushes to
        // `objects` and `forwarding` together and the minor collector indexes
        // both by nursery index, so a realloc in either is a moving backing
        // store — which a planned JIT inline bump and a planned
        // `Heap::alloc_str_concat_inline` (JIT string-codegen plan, Tasks 2
        // and 5; neither exists yet) will need to assume cannot happen. Fixed
        // size, so this is ~900 KB paid once rather than a growth curve —
        // for the nursery a live `HeapInner` owns. See `vacant` for the
        // placeholder used when a `Nursery` is briefly swapped out, which
        // must NOT pay this cost.
        Self {
            objects: Vec::with_capacity(NURSERY_CAPACITY),
            forwarding: Vec::with_capacity(NURSERY_CAPACITY),
            remembered: Vec::new(),
            worklist: Vec::new(),
            phase: "?",
            scan_candidates: Vec::new(),
            alloc_count: 0,
            minor_gc_count: 0,
            minor_gc_promoted: 0,
        }
    }

    /// An empty, non-allocating placeholder — capacity 0 in both `objects`
    /// and `forwarding`, so construction touches no allocator.
    ///
    /// For use only as a swap target while the real nursery is moved out
    /// (see `HeapInner::minor_gc`). `Default`/`new` reserve
    /// `NURSERY_CAPACITY` up front (~900 KB) so that a *live* nursery never
    /// reallocates; `mem::take`, which builds a `Default`, would pay that
    /// same ~900 KB on every single minor GC to build a value that is
    /// dropped a few lines later. `Vec::new()` is documented not to
    /// allocate until pushed to, so this constructor is zero-cost; see
    /// `capacity_invariant::vacant_nursery_allocates_nothing`.
    pub(crate) fn vacant() -> Self {
        Self {
            objects: Vec::new(),
            forwarding: Vec::new(),
            remembered: Vec::new(),
            worklist: Vec::new(),
            phase: "?",
            scan_candidates: Vec::new(),
            alloc_count: 0,
            minor_gc_count: 0,
            minor_gc_promoted: 0,
        }
    }

    #[inline(always)]
    pub(crate) fn try_alloc(&mut self, obj: HeapObj) -> Result<u32, HeapObj> {
        if self.objects.len() >= NURSERY_CAPACITY {
            return Err(obj);
        }
        let idx = self.objects.len() as u32;
        self.objects.push(Some(obj));
        self.forwarding.push(None);
        self.alloc_count += 1;
        Ok(idx)
    }

    #[inline(always)]
    pub(crate) fn get(&self, idx: u32) -> Option<&HeapObj> {
        self.objects.get(idx as usize)?.as_ref()
    }

    #[inline(always)]
    pub(crate) fn get_mut(&mut self, idx: u32) -> Option<&mut HeapObj> {
        self.objects.get_mut(idx as usize)?.as_mut()
    }

    #[inline(always)]
    pub(crate) fn is_full(&self) -> bool {
        self.objects.len() >= Self::FULL_THRESHOLD
    }

    /// Fill level at which [`is_full`] reports true. Exposed so the JIT
    /// back-edge safepoint compares against the same limit.
    pub const FULL_THRESHOLD: usize = NURSERY_CAPACITY * 3 / 4;

    /// Byte offset of the live-object count (`objects.len()`) inside
    /// `Nursery`, for the JIT back-edge safepoint. Relies on Vec's
    /// (cap, ptr, len) word layout — the same assumption the JIT already
    /// makes when it reads `ExecCtx.stack`/`ExecCtx.frames` lengths.
    pub(crate) fn objects_len_byte_offset() -> usize {
        std::mem::offset_of!(Nursery, objects) + 2 * std::mem::size_of::<usize>()
    }

    /// Byte offset of the `objects` Vec's three words within `Nursery`,
    /// for the JIT's inline array-read fast path.
    pub(crate) fn objects_vec_byte_offset() -> usize {
        std::mem::offset_of!(Nursery, objects)
    }

    /// Byte offset of the `forwarding` Vec's three words within `Nursery`,
    /// for the JIT's inline allocation — which must bump both Vecs, since the
    /// minor collector indexes them together.
    pub(crate) fn forwarding_vec_byte_offset() -> usize {
        std::mem::offset_of!(Nursery, forwarding)
    }

    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        self.objects.len()
    }

    /// Duplicates are allowed here — the write barrier is the hot path, so
    /// dedup happens once per collection instead of O(n) per store.
    #[inline(always)]
    pub(crate) fn remember(&mut self, packed_old_idx: u32) {
        self.remembered.push(packed_old_idx);
    }

    pub(crate) fn collect(
        &mut self,
        old_gen: &mut HeapInner,
        stack: &mut [VmValue],
        extra_root_packed: &[u32],
    ) {
        self.minor_gc_count += 1;
        let mut worklist = std::mem::take(&mut self.worklist);
        worklist.clear();
        // Reused across every scanned object: one promoted object per scan
        // previously meant one fresh Vec allocation each.
        let mut fixups: Vec<(ChildSlot, u32)> = Vec::with_capacity(8);

        self.phase = "stack/ctx roots";
        for slot in stack.iter_mut() {
            self.update_value(slot, old_gen, &mut worklist);
        }

        let mut old_indices_to_scan = std::mem::take(&mut self.remembered);
        old_indices_to_scan.sort_unstable();
        old_indices_to_scan.dedup();
        self.phase = "remembered set (barrera old->young)";
        for packed in old_indices_to_scan {
            self.scan_and_fix_old_obj(old_idx_raw(packed), old_gen, &mut worklist, &mut fixups);
        }

        // Closures/classes/modules hold Rust-side `Value`s the write barrier
        // does not cover; scan only the tracked candidates instead of the
        // whole old gen (which made every minor GC O(old-gen size)).
        let mut candidates = std::mem::take(&mut self.scan_candidates);
        candidates.clear();
        candidates.extend_from_slice(old_gen.scan_roots());
        self.phase = "scan_roots del old gen";
        for &raw_idx in &candidates {
            if matches!(old_gen.get_raw(raw_idx), Some(obj) if Self::can_reference_nursery(obj)) {
                self.scan_and_fix_old_obj(raw_idx, old_gen, &mut worklist, &mut fixups);
            }
        }
        self.scan_candidates = candidates;

        self.phase = "extra roots";
        for &packed in extra_root_packed {
            if is_old_idx(packed) {
                self.scan_and_fix_old_obj(old_idx_raw(packed), old_gen, &mut worklist, &mut fixups);
            } else {
                self.evacuate(packed, old_gen, &mut worklist);
            }
        }

        self.phase = "worklist (hijos de lo ya promovido)";
        while let Some(raw) = worklist.pop() {
            self.scan_and_fix_old_obj(raw, old_gen, &mut worklist, &mut fixups);
        }

        old_gen.update_interners_after_minor_gc(&self.forwarding);

        self.objects.clear();
        self.forwarding.clear();
        self.worklist = worklist;
    }

    #[inline]
    fn update_value(
        &mut self,
        val: &mut VmValue,
        old_gen: &mut HeapInner,
        worklist: &mut Vec<u32>,
    ) {
        if !val.is_heap() {
            return;
        }
        let idx = val.as_heap_idx();
        if !is_nursery_idx(idx) {
            return;
        }
        let packed = self.evacuate(idx, old_gen, worklist);
        *val = VmValue::from_heap_idx(packed);
    }

    fn evacuate(
        &mut self,
        nursery_idx: u32,
        old_gen: &mut HeapInner,
        worklist: &mut Vec<u32>,
    ) -> u32 {
        if let Some(Some(fwd)) = self.forwarding.get(nursery_idx as usize) {
            return *fwd;
        }
        let obj = match self.objects.get_mut(nursery_idx as usize) {
            Some(slot @ &mut Some(_)) => slot.take().unwrap(),
            // Llegar aquí significa que algo conserva un índice de nursery que
            // ya no existe: casi siempre una referencia old→young que la
            // barrera de escritura no registró.
            //
            // El resultado tiene que ser un índice VÁLIDO. Devolver uno
            // imposible (`u32::MAX`) para que fallase pronto convierte esto en
            // un SEGFAULT: el código compilado resuelve handles del heap sin
            // comprobar límites, así que un índice fuera de rango revienta el
            // proceso en vez de dar un error de VM. Medido, no supuesto —
            // `bench_http_routing` con 95 000 peticiones lo hace.
            //
            // Así que el valor se queda, pero el suceso deja de ser mudo: sin
            // este aviso el programa sigue con un objeto ajeno en la mano y el
            // fallo aparece mucho después y en otro sitio (un `GetFixedField`
            // sobre la clase `Error`, sin nada que lo ligue a la colección que
            // lo causó). Ver `bench-jit-snapshot-corruption` en las notas del
            // proyecto: la causa raíz sigue abierta.
            _ => {
                debug_assert!(
                    false,
                    "evacuate: índice de nursery {nursery_idx} sin objeto — \
                     referencia old→young no registrada por la barrera"
                );
                // Una vez por proceso: el caso se da dentro de una colección,
                // y una colección puede encontrar miles de referencias al
                // mismo objeto perdido.
                static WARNED: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    eprintln!(
                        "warning[gc]: referencia colgante al nursery ({nursery_idx}) durante la \
                         colección menor, fase «{}» — una referencia old→young no quedó \
                         registrada por la barrera de escritura. Los valores leídos a través de \
                         ella serán incorrectos. (Sólo se avisa una vez.)",
                        self.phase
                    );
                }
                return pack_old_idx(0);
            }
        };
        let raw_old = old_gen.alloc_raw(obj);
        let packed = pack_old_idx(raw_old);
        if let Some(slot) = self.forwarding.get_mut(nursery_idx as usize) {
            *slot = Some(packed);
        }
        // Contado aquí, que es donde la promoción ocurre. Derivarlo al final
        // contando entradas en `forwarding` recorría toda la nursery (hasta
        // 49 152 ranuras) en cada colección para una sola estadística.
        self.minor_gc_promoted += 1;
        worklist.push(raw_old);
        packed
    }

    fn scan_and_fix_old_obj(
        &mut self,
        raw_old: u32,
        old_gen: &mut HeapInner,
        worklist: &mut Vec<u32>,
        fixups: &mut Vec<(ChildSlot, u32)>,
    ) {
        fixups.clear();

        // ONE lookup, whatever the variant. This runs once per promoted
        // object, and probing `get_raw` separately for Generator, then Map,
        // then Set, then everything else charged four bounds-checked slot
        // reads to reach the common case (a plain Object or Array). The
        // three container variants still have to hand their handle out of
        // the borrow — they evacuate THROUGH `old_gen`, so its borrow must
        // end first — but every other variant is scanned right here.
        let deferred = match old_gen.get_raw(raw_old) {
            Some(HeapObj::Generator(g)) => Container::Generator(g.0.clone()),
            Some(HeapObj::Map(m)) => Container::Map(m.clone()),
            Some(HeapObj::Set(s)) => Container::Set(s.clone()),
            Some(obj) => {
                Self::scan_children(obj, old_gen, fixups);
                Container::None
            }
            None => Container::None,
        };

        match deferred {
            // A generator carries a whole suspended ExecCtx (stack, frames,
            // upvalues, pending suspends) whose slots hold raw heap indices;
            // rewrite them in place through the driver's mutable trace.
            Container::Generator(driver) => {
                driver.trace_vm_values_mut(&mut |val| {
                    self.update_value(val, old_gen, worklist);
                });
                return;
            }
            // Map/Set entries are raw VmValues mutated through interior
            // mutability (the write barrier remembers the collection).
            // Values rewrite in place; canonical keys (interned strings,
            // scalars) are old-gen-stable, but identity keys can move —
            // their hash is their bit pattern, so the table is rebuilt when
            // any key evacuates.
            Container::Map(m) => {
                let mut g = m.borrow_mut();
                for v in g.values_mut() {
                    self.update_value(v, old_gen, worklist);
                }
                let any_key_moved = g
                    .keys()
                    .any(|k| k.0.is_heap() && is_nursery_idx(k.0.as_heap_idx()));
                if any_key_moved {
                    let entries: Vec<(VmValue, VmValue)> =
                        g.drain().map(|(k, v)| (k.0, v)).collect();
                    for (mut k, v) in entries {
                        self.update_value(&mut k, old_gen, worklist);
                        g.insert(varn_types::value::MapKey(k), v);
                    }
                }
                return;
            }
            Container::Set(s) => {
                let mut g = s.borrow_mut();
                let any_key_moved = g
                    .iter()
                    .any(|k| k.0.is_heap() && is_nursery_idx(k.0.as_heap_idx()));
                if any_key_moved {
                    let items: Vec<VmValue> = g.drain().map(|k| k.0).collect();
                    for mut k in items {
                        self.update_value(&mut k, old_gen, worklist);
                        g.insert(varn_types::value::MapKey(k));
                    }
                }
                return;
            }
            Container::None => {}
        }

        for (slot, nursery_idx) in fixups.drain(..) {
            let packed = self.evacuate(nursery_idx, old_gen, worklist);
            let new_val = VmValue::from_heap_idx(packed);
            let extracted_val = if matches!(slot, ChildSlot::BoundMethodReceiver)
                || matches!(slot, ChildSlot::ClassVtableItem(_))
                || matches!(slot, ChildSlot::ClassStatic(_))
                || matches!(slot, ChildSlot::EnumVariantPayload)
            {
                Some(old_gen.extract(new_val))
            } else {
                None
            };
            let Some(obj) = old_gen.get_raw_mut(raw_old) else {
                continue;
            };
            match (slot, obj) {
                (ChildSlot::ArrayItem(i), HeapObj::Array(arr)) => {
                    // `ArrayItem` fixups are only ever pushed for a `Boxed`
                    // array (see the scan above), but use the total accessor
                    // rather than `borrow_mut()` anyway — never assume a
                    // repr invariant here that isn't locally re-checked.
                    if let Some(v) = arr.as_boxed_mut() {
                        if let Some(s) = v.get_mut(i) {
                            *s = new_val;
                        }
                    }
                }
                (ChildSlot::ObjField(i), HeapObj::Object(obj_ref)) => {
                    obj_ref.set_field_at(i, new_val);
                }
                (ChildSlot::Upvalue(i), HeapObj::VmClosure(clos)) => {
                    if let Some(uv) = clos.upvalues.get(i) {
                        if let Ok(mut inner) = uv.inner.try_borrow_mut() {
                            inner.value = new_val;
                        }
                    }
                }
                (ChildSlot::Spread, HeapObj::Spread(v)) => {
                    *v = new_val;
                }
                (ChildSlot::BoundMethodReceiver, HeapObj::BoundMethod(bm)) => {
                    bm.receiver = extracted_val.unwrap();
                }
                (ChildSlot::ClassVtableItem(i), HeapObj::Class(cls)) => {
                    cls.vtable.borrow_mut()[i] = extracted_val.unwrap();
                }
                (ChildSlot::ClassStatic(k), HeapObj::Class(cls)) => {
                    cls.statics.borrow_mut().insert(k, extracted_val.unwrap());
                }
                (ChildSlot::ModuleExport(i), HeapObj::Module(m)) => {
                    if let Some(s) = Rc::make_mut(m).exports.get_mut(i) {
                        *s = new_val;
                    }
                }
                (ChildSlot::EnumVariantPayload, HeapObj::EnumVariant(ev)) => {
                    ev.payload = extracted_val.unwrap();
                }
                _ => {}
            }
        }
    }

    /// Record every child of `obj` that still points into the nursery. Pure
    /// scan: it only reads through the borrow of `old_gen` the caller holds,
    /// so the evacuation that consumes `fixups` happens after it returns.
    /// Whether [`Self::scan_children`] can produce a fixup for `obj` — i.e.
    /// whether this kind of object can hold a reference to a nursery object.
    ///
    /// An old-generation object is only visited by the minor collector if it is
    /// in `scan_roots` or was caught by the write barrier. An object BORN in the
    /// old generation — which happens whenever the nursery is full at the moment
    /// it is allocated — goes through neither, so it must be enrolled at birth,
    /// and this is the predicate that decides.
    ///
    /// It must agree with `scan_children` below. It did not: `Array`, `Object`,
    /// `Spread` and `EnumVariant` were handled there and missing here. A
    /// `JSON.parse` of 50 000 objects fills the nursery, the result array lands
    /// in the old generation holding nursery elements, and the next minor
    /// collection evacuates those elements without updating it — the array is
    /// left pointing at freed slots and the following read panics with
    /// "dangling or corrupted heap reference".
    ///
    /// Keep the two matches in the same file, and in the same order, so a new
    /// `HeapObj` variant cannot be added to one alone.
    pub(crate) fn can_reference_nursery(obj: &HeapObj) -> bool {
        matches!(
            obj,
            HeapObj::Array(_)
                | HeapObj::Tuple(_)
                | HeapObj::Object(_)
                | HeapObj::Record(_)
                | HeapObj::VmClosure(_)
                | HeapObj::Spread(_)
                | HeapObj::BoundMethod(_)
                | HeapObj::Class(_)
                | HeapObj::Module(_)
                | HeapObj::EnumVariant(_)
                | HeapObj::Generator(_)
                | HeapObj::Map(_)
                | HeapObj::Set(_)
        )
    }

    /// Whether `obj` points at a nursery object right now.
    ///
    /// The write barrier catches an old-generation object being WRITTEN with a
    /// nursery reference, but nothing catches one being BORN holding them —
    /// which happens to every object allocated while the nursery is full. Those
    /// belong in `remembered` at birth; this is the test, applied once, at
    /// allocation. See `HeapInner::alloc`.
    pub(crate) fn holds_nursery_ref(obj: &HeapObj) -> bool {
        let nursery_val = |v: &VmValue| v.is_heap() && is_nursery_idx(v.as_heap_idx());
        match obj {
            HeapObj::Array(arr) | HeapObj::Tuple(arr) => match arr.as_boxed() {
                Some(items) => items.iter().any(nursery_val),
                None => false,
            },
            HeapObj::Object(o) | HeapObj::Record(o) => {
                let mut found = false;
                o.borrow().for_each_field(|_, v| {
                    found |= nursery_val(&v);
                });
                found
            }
            HeapObj::Spread(v) => nursery_val(v),
            // An enum variant's payload and a map/set's entries are Rust-side
            // `Value`s; deciding cheaply would mean duplicating their traversal.
            // They are rare enough that always enrolling is the honest choice.
            HeapObj::EnumVariant(_) | HeapObj::Map(_) | HeapObj::Set(_) => true,
            _ => false,
        }
    }

    fn scan_children(obj: &HeapObj, old_gen: &HeapInner, fixups: &mut Vec<(ChildSlot, u32)>) {
        match obj {
            HeapObj::Array(arr) | HeapObj::Tuple(arr) => {
                // Variant-aware: I64/F64 elements are raw numeric words,
                // never a nursery heap ref, so there is nothing to scan
                // or fix up. Only `Boxed` can hold a moved child.
                if let Some(items) = arr.as_boxed() {
                    for (i, &v) in items.iter().enumerate() {
                        if v.is_heap() && is_nursery_idx(v.as_heap_idx()) {
                            fixups.push((ChildSlot::ArrayItem(i), v.as_heap_idx()));
                        }
                    }
                }
            }
            HeapObj::Object(obj_ref) | HeapObj::Record(obj_ref) => {
                obj_ref.borrow().for_each_field(|i, v| {
                    if v.is_heap() && is_nursery_idx(v.as_heap_idx()) {
                        fixups.push((ChildSlot::ObjField(i), v.as_heap_idx()));
                    }
                });
            }
            HeapObj::VmClosure(clos) => {
                for (i, uv) in clos.upvalues.iter().enumerate() {
                    if let Ok(inner) = uv.inner.try_borrow() {
                        if inner.value.is_heap() && is_nursery_idx(inner.value.as_heap_idx()) {
                            fixups.push((ChildSlot::Upvalue(i), inner.value.as_heap_idx()));
                        }
                    }
                }
            }
            HeapObj::Spread(v) => {
                if v.is_heap() && is_nursery_idx(v.as_heap_idx()) {
                    fixups.push((ChildSlot::Spread, v.as_heap_idx()));
                }
            }
            HeapObj::BoundMethod(bm) => {
                if let Some(idx) = old_gen.value_heap_idx(&bm.receiver) {
                    if is_nursery_idx(idx) {
                        fixups.push((ChildSlot::BoundMethodReceiver, idx));
                    }
                }
            }
            HeapObj::Class(cls) => {
                for (i, v) in cls.vtable.borrow().iter().enumerate() {
                    if let Some(idx) = old_gen.value_heap_idx(v) {
                        if is_nursery_idx(idx) {
                            fixups.push((ChildSlot::ClassVtableItem(i), idx));
                        }
                    }
                }
                for (k, v) in cls.statics.borrow().iter() {
                    if let Some(idx) = old_gen.value_heap_idx(v) {
                        if is_nursery_idx(idx) {
                            fixups.push((ChildSlot::ClassStatic(k.clone()), idx));
                        }
                    }
                }
            }
            HeapObj::Module(m) => {
                for (i, &v) in m.exports.iter().enumerate() {
                    if v.is_heap() && is_nursery_idx(v.as_heap_idx()) {
                        fixups.push((ChildSlot::ModuleExport(i), v.as_heap_idx()));
                    }
                }
            }
            HeapObj::EnumVariant(ev) => {
                if let Some(idx) = old_gen.value_heap_idx(&ev.payload) {
                    if is_nursery_idx(idx) {
                        fixups.push((ChildSlot::EnumVariantPayload, idx));
                    }
                }
            }
            _ => {}
        }
    }
}

/// The variants `scan_and_fix_old_obj` cannot scan under the borrow that
/// found them: each rewrites its contents by evacuating THROUGH `old_gen`,
/// so the handle has to leave the borrow first. Everything else is scanned
/// in place by `scan_children` and reports `None` here.
enum Container {
    Generator(Rc<dyn varn_types::generator::GeneratorDriver>),
    Map(varn_types::value::MapRef),
    Set(varn_types::value::SetRef),
    None,
}

#[derive(Clone)]
enum ChildSlot {
    ArrayItem(usize),
    ObjField(usize),
    Upvalue(usize),
    Spread,
    BoundMethodReceiver,
    ClassVtableItem(usize),
    ClassStatic(std::rc::Rc<str>),
    ModuleExport(usize),
    EnumVariantPayload,
}
