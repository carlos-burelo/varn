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

    /// Every live object currently in the nursery — for `vn debug -p gc`'s
    /// histogram, not a hot path.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &HeapObj> {
        self.objects.iter().filter_map(|o| o.as_ref())
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
        if self.remembered.last().copied() != Some(packed_old_idx) {
            self.remembered.push(packed_old_idx);
        }
    }
}

mod collect;
mod scan;
