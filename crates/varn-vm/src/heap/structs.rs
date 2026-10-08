use super::cells::CellSpace;
use super::obj::HeapObj;
use super::young::YoungGen;
use crate::profile::HotspotCounters;
use crate::value::VmValue;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;
use varn_types::HeapRef;
use varn_types::{value::RuntimeSymbol, ClassObj, RuntimeString};

pub struct HeapInner {
    pub jit_epoch: u64,
    pub intrinsic_classes: FxHashMap<String, Rc<ClassObj>>,
    pub gc_collections: u64,
    pub gc_total_freed: u64,
    pub gc_threshold: u64,
    pub(crate) young: YoungGen,
    pub(crate) cells: CellSpace,
    pub(super) string_interner: FxHashMap<RuntimeString, HeapRef>,
    pub(super) symbol_interner: FxHashMap<RuntimeSymbol, HeapRef>,
    pub(crate) young_cells: Vec<Rc<crate::task::TaskCell>>,
    pub(crate) young_lazies: Vec<Rc<crate::task::LazyTask>>,
    pub(super) bigint_interner: FxHashMap<num_bigint::BigInt, HeapRef>,
    pub(super) decimal_interner: FxHashMap<bigdecimal::BigDecimal, HeapRef>,
    pub(super) char_interner: FxHashMap<char, HeapRef>,
    pub(super) major_work: Vec<HeapRef>,
    pub hotspot: Option<Rc<RefCell<HotspotCounters>>>,
    pub(super) scan_roots: Vec<HeapRef>,
    pub(super) identity_index: FxHashMap<usize, HeapRef>,
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
            cells: CellSpace::default(),
            intrinsic_classes: FxHashMap::default(),
            string_interner: FxHashMap::default(),
            symbol_interner: FxHashMap::default(),
            young_cells: Vec::new(),
            young_lazies: Vec::new(),
            bigint_interner: FxHashMap::default(),
            decimal_interner: FxHashMap::default(),
            char_interner: FxHashMap::default(),
            major_work: Vec::new(),
            gc_collections: 0,
            gc_total_freed: 0,
            gc_threshold: 65536,
            young: YoungGen::default(),
            hotspot: None,
            scan_roots: Vec::new(),
            identity_index: FxHashMap::default(),
        }
    }

    #[inline]
    pub(super) fn identity_key(obj: &HeapObj) -> Option<usize> {
        match obj {
            HeapObj::Class(c) => Some(Rc::as_ptr(c) as usize),
            HeapObj::Generator(g) => Some(Rc::as_ptr(&g.0) as *const () as usize),
            HeapObj::VmClosure(c) => Some(Rc::as_ptr(c) as usize),
            HeapObj::Str(_) | HeapObj::Array(_) | HeapObj::Tuple(_) | HeapObj::Object(_) | HeapObj::Record(_) | HeapObj::Buffer(_) | HeapObj::Module(_) | HeapObj::FrozenModule(_) | HeapObj::NativeFn(..) | HeapObj::BoundMethod(_) | HeapObj::Map(_) | HeapObj::Set(_) | HeapObj::Task(_) | HeapObj::TaskHandle(_) | HeapObj::Range(_) | HeapObj::Symbol(_) | HeapObj::EnumVariant(_) | HeapObj::BigInt(_) | HeapObj::Decimal(_) | HeapObj::Char(_) | HeapObj::Spread(_) => None,
        }
    }

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

    #[inline(always)]
    pub(super) fn born_old(obj: &HeapObj) -> bool {
        matches!(
            obj,
            HeapObj::Class(_)
                | HeapObj::Module(_)
                | HeapObj::FrozenModule(_)
                | HeapObj::NativeFn(..)
                | HeapObj::Generator(_)
                | HeapObj::Task(_)
                | HeapObj::TaskHandle(_)
        )
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

    pub(crate) fn objects_len(&self) -> usize {
        self.cells.capacity()
    }

    pub(crate) fn alloc_count(&self) -> u64 {
        self.young.alloc_count() + self.cells.old_births
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

    #[inline(always)]
    pub(crate) fn jit_epoch(&self) -> u64 {
        unsafe { (*self.inner.get()).jit_epoch }
    }

    #[allow(clippy::mut_from_ref)]
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
        write!(f, "Heap {{ alloc_count: {} }}", self.alloc_count())
    }
}
