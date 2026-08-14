use super::obj::HeapObj;
use crate::gc::GcCollector;
use crate::nursery::Nursery;
use crate::profile::HotspotCounters;
use crate::value::VmValue;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;
use varn_types::{
    value::{ArrayRef, MapRef, ObjRef, RuntimeSymbol, SetRef},
    ClassObj, RuntimeString,
};

#[derive(Clone)]
pub struct HeapInner {
    /// Identity of this object table, and therefore of every `VmValue` handle
    /// into it. Compiled code bakes those handles as immediates, so it is only
    /// valid while this heap is: see `FunctionProto::jit_epoch`. Shared by
    /// `Heap::clone` (a nested context reaches the same objects) and fresh for
    /// `deep_clone` (equal contents, separate table).
    pub jit_epoch: u64,
    /// Epochs this heap was copied from, each with the compile serial at the
    /// moment of the copy. A `deep_clone` duplicates the whole table —
    /// same indices, same contents, same interner — so code an ancestor
    /// compiled BEFORE the copy baked handles this heap also has, and stays
    /// valid here. Code compiled after the copy did not: that is what sibling
    /// clones (the bench's runs) must never share, and the whole point of the
    /// epoch. Ordered oldest first; typically empty or one entry.
    pub jit_ancestry: Vec<(u64, u64)>,
    pub alloc_count: u64,
    pub intrinsic_classes: FxHashMap<String, Rc<ClassObj>>,
    pub gc_collections: u64,
    pub gc_total_freed: u64,
    pub gc_alloc_since_collect: u64,
    pub nursery: Nursery,
    pub(super) free: Vec<u32>,
    pub(super) objects: Vec<Option<HeapObj>>,
    pub(super) string_interner: FxHashMap<RuntimeString, u32>,
    pub(super) symbol_interner: FxHashMap<RuntimeSymbol, u32>,
    pub(super) array_interner: FxHashMap<ArrayRef, u32>,
    pub(super) object_interner: FxHashMap<ObjRef, u32>,
    pub(super) map_interner: FxHashMap<MapRef, u32>,
    pub(super) set_interner: FxHashMap<SetRef, u32>,
    pub(super) bigint_interner: FxHashMap<i128, u32>,
    pub(super) decimal_interner: FxHashMap<rust_decimal::Decimal, u32>,
    pub(super) char_interner: FxHashMap<char, u32>,
    pub(super) gc_collector: Option<GcCollector>,
    pub hotspot: Option<Rc<RefCell<HotspotCounters>>>,
    pub(super) scan_roots: Vec<u32>,
    pub(super) identity_index: FxHashMap<usize, u32>,
}

impl Drop for HeapInner {
    fn drop(&mut self) {
        crate::clif_link::invalidate_epoch(self.jit_epoch);
    }
}

impl HeapInner {
    pub(crate) fn new() -> Self {
        Self {
            jit_epoch: crate::clif_link::next_epoch(),
            jit_ancestry: Vec::new(),
            objects: Vec::with_capacity(4096),
            free: Vec::new(),
            alloc_count: 0,
            intrinsic_classes: FxHashMap::default(),
            string_interner: FxHashMap::default(),
            symbol_interner: FxHashMap::default(),
            array_interner: FxHashMap::default(),
            object_interner: FxHashMap::default(),
            map_interner: FxHashMap::default(),
            set_interner: FxHashMap::default(),
            bigint_interner: FxHashMap::default(),
            decimal_interner: FxHashMap::default(),
            char_interner: FxHashMap::default(),
            gc_collector: Some(GcCollector::new(4096)),
            gc_collections: 0,
            gc_total_freed: 0,
            gc_alloc_since_collect: 0,
            nursery: Nursery::new(),
            hotspot: None,
            scan_roots: Vec::new(),
            identity_index: FxHashMap::default(),
        }
    }

    #[inline]
    pub(super) fn identity_key(obj: &HeapObj) -> Option<usize> {
        match obj {
            HeapObj::Class(c) => Some(Rc::as_ptr(c) as usize),
            HeapObj::Task(t) => Some(Rc::as_ptr(t) as usize),
            HeapObj::TaskHandle(th) => Some(th.identity()),
            HeapObj::Generator(g) => Some(Rc::as_ptr(&g.0) as *const () as usize),
            HeapObj::AsyncQueue(q) => Some(Rc::as_ptr(&q.0) as usize),
            HeapObj::VmClosure(c) => Some(Rc::as_ptr(c) as usize),
            _ => None,
        }
    }

    /// Whether an object belongs in `scan_roots` — the set the minor collector
    /// re-walks on EVERY collection, for as long as the object lives.
    ///
    /// Only for kinds holding Rust-side `Value`s that no write barrier covers.
    /// Containers are covered by the barrier and must not go here: `scan_roots`
    /// is never pruned, so enrolling them permanently makes each minor
    /// collection O(containers ever born). A container born already pointing
    /// into the nursery is registered in the nursery's `remembered` set
    /// instead, which is drained every collection — see `HeapInner::alloc`.
    #[inline(always)]
    pub(super) fn needs_minor_scan(obj: &HeapObj) -> bool {
        matches!(
            obj,
            HeapObj::VmClosure(_)
                | HeapObj::BoundMethod(_)
                | HeapObj::Class(_)
                | HeapObj::Module(_)
                | HeapObj::Generator(_)
        )
    }

    pub(crate) fn scan_roots(&self) -> &[u32] {
        &self.scan_roots
    }

    #[inline(always)]
    pub(crate) fn is_int(&self, v: VmValue) -> bool {
        v.is_int()
    }

    #[inline(always)]
    pub(crate) fn as_int(&self, v: VmValue) -> i64 {
        if v.is_int() {
            v.as_int()
        } else {
            0
        }
    }

    #[inline(always)]
    pub(crate) fn to_f64_val(&self, v: VmValue) -> f64 {
        if v.is_f64() {
            v.as_f64()
        } else if v.is_int() {
            v.as_int() as f64
        } else {
            0.0
        }
    }

    pub(crate) fn set_intrinsic_class(&mut self, name: &str, cls: Rc<ClassObj>) {
        self.intrinsic_classes.insert(name.to_string(), cls);
    }

    pub(crate) fn get_intrinsic_class(&self, name: &str) -> Option<Rc<ClassObj>> {
        self.intrinsic_classes.get(name).cloned()
    }

    pub(crate) fn objects_len(&self) -> u32 {
        self.objects.len() as u32
    }

    pub(crate) fn objects(&self) -> &Vec<Option<HeapObj>> {
        &self.objects
    }

    pub(crate) fn objects_mut(&mut self) -> &mut Vec<Option<HeapObj>> {
        &mut self.objects
    }

    pub(crate) fn free_list_mut(&mut self) -> &mut Vec<u32> {
        &mut self.free
    }

    pub(crate) fn identity_index(&self) -> &FxHashMap<usize, u32> {
        &self.identity_index
    }
}

#[derive(Clone)]
pub struct Heap {
    pub(super) inner: Rc<std::cell::UnsafeCell<HeapInner>>,
}

impl Heap {
    pub(crate) fn new() -> Self {
        Self {
            inner: Rc::new(std::cell::UnsafeCell::new(HeapInner::new())),
        }
    }

    pub(crate) fn deep_clone(&self) -> Self {
        let mut inner_clone = unsafe { (*self.inner.get()).clone() };
        let parent = inner_clone.jit_epoch;
        inner_clone
            .jit_ancestry
            .push((parent, crate::clif_link::compile_serial()));
        inner_clone.jit_epoch = crate::clif_link::next_epoch();
        Self {
            inner: Rc::new(std::cell::UnsafeCell::new(inner_clone)),
        }
    }

    #[inline(always)]
    pub(crate) fn jit_epoch(&self) -> u64 {
        unsafe { (*self.inner.get()).jit_epoch }
    }

    pub(crate) fn jit_ancestry(&self) -> Vec<(u64, u64)> {
        unsafe { (*self.inner.get()).jit_ancestry.clone() }
    }

    #[inline(always)]
    pub(crate) unsafe fn inner_mut(&self) -> &mut HeapInner {
        &mut *self.inner.get()
    }
}

impl std::ops::Deref for Heap {
    type Target = HeapInner;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.inner.get() }
    }
}

impl std::ops::DerefMut for Heap {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.inner.get() }
    }
}

impl std::fmt::Debug for Heap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Heap {{ alloc_count: {} }}", self.alloc_count)
    }
}
