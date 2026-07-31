use crate::error::{RuntimeError, VmResult};
use crate::frame::{VmClosure, VmClosurePayload, VmValueRef};
use crate::gc::GcCollector;
use crate::nursery::{is_nursery_idx, is_old_idx, old_idx_raw, pack_old_idx, Nursery};
use crate::profile::HotspotCounters;
use crate::value::VmValue;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use std::sync::Arc;
use varn_base::VmValuePayload;
use varn_types::{
    generator::{AsyncQueue, GeneratorObj},
    value::{
        ArrayRef, BoundMethod, EnumVariantData, FrozenModuleObj, MapRef, ModuleObj, ObjRef,
        RangeData, RuntimeSymbol, SetRef,
    },
    AsyncTask, ClassObj, LazyTask, NativeCtx, NativeFn, ResourceStore, RuntimeString,
    Value, VmArray,
};

/// Lazily-computed ASCII cache for a `HeapStr`. The viewed content of a
/// `HeapStr` is immutable (a `Shared` string is frozen; an `Ext` prefix view
/// never changes once created), so once computed the answer is stable for the
/// lifetime of the instance.
pub mod ascii_flag {
    pub const UNKNOWN: u8 = 0;
    pub const YES: u8 = 1;
    pub const NO: u8 = 2;
}

/// Byte length limit for `HeapStr::Slice` (30 bits); the two high bits of
/// `len_ascii` hold the ASCII flag so the variant stays within the enum's
/// existing payload size.
const SLICE_LEN_MASK: u32 = 0x3FFF_FFFF;

/// Heap string payload. `Shared` is an immutable interned/frozen string;
/// `Ext` is a prefix view (`buf[..len]`) of a shared growable buffer, the
/// representation `str_concat` produces for accumulation patterns. Appending
/// to the buffer's tip never changes an existing prefix, so older views stay
/// valid without any aliasing analysis; a view that is no longer the tip is
/// copied out before growing. Single-threaded interior mutability via
/// `UnsafeCell` mirrors `ArrayRef`. `Slice` is a zero-copy substring view of
/// an immutable `Shared` buffer (`src[off..off+len]`), the representation
/// `substring`/`slice` produce; note it retains the whole source buffer for
/// its lifetime (the JS-engine substring-view trade-off).
#[derive(Clone)]
pub enum HeapStr {
    Shared(RuntimeString, std::cell::Cell<u8>),
    Ext {
        buf: Rc<std::cell::UnsafeCell<String>>,
        len: usize,
        ascii: std::cell::Cell<u8>,
    },
    Slice {
        src: RuntimeString,
        off: u32,
        /// bits 0..30 = byte length, bits 30..32 = `ascii_flag` state.
        len_ascii: std::cell::Cell<u32>,
    },
}

impl HeapStr {
    #[inline]
    pub fn shared(s: RuntimeString) -> Self {
        HeapStr::Shared(s, std::cell::Cell::new(ascii_flag::UNKNOWN))
    }

    #[inline]
    pub fn ext(buf: Rc<std::cell::UnsafeCell<String>>, len: usize, ascii: u8) -> Self {
        HeapStr::Ext {
            buf,
            len,
            ascii: std::cell::Cell::new(ascii),
        }
    }

    /// Zero-copy substring view over an immutable shared buffer. Caller
    /// guarantees `off..off+len` lies on char boundaries and `len` fits
    /// [`SLICE_LEN_MASK`].
    #[inline]
    pub fn slice_of(src: RuntimeString, off: usize, len: usize, ascii: u8) -> Self {
        debug_assert!(len as u32 <= SLICE_LEN_MASK);
        HeapStr::Slice {
            src,
            off: off as u32,
            len_ascii: std::cell::Cell::new(len as u32 | ((ascii as u32) << 30)),
        }
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        match self {
            HeapStr::Shared(s, _) => s,
            // Safety: single-threaded VM; the buffer is only appended to (via
            // `str_concat`), never while a borrow from this view is live.
            HeapStr::Ext { buf, len, .. } => unsafe { &(&*buf.get())[..*len] },
            HeapStr::Slice { src, off, len_ascii } => {
                let off = *off as usize;
                let len = (len_ascii.get() & SLICE_LEN_MASK) as usize;
                &src[off..off + len]
            }
        }
    }

    /// Materialize as an `Rc<str>` (copies `Ext` and `Slice` views).
    pub fn to_shared(&self) -> RuntimeString {
        match self {
            HeapStr::Shared(s, _) => Rc::clone(s),
            _ => Rc::from(self.as_str()),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        match self {
            HeapStr::Shared(s, _) => s.len(),
            HeapStr::Ext { len, .. } => *len,
            HeapStr::Slice { len_ascii, .. } => (len_ascii.get() & SLICE_LEN_MASK) as usize,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn ascii_state(&self) -> u8 {
        match self {
            HeapStr::Shared(_, ascii) => ascii.get(),
            HeapStr::Ext { ascii, .. } => ascii.get(),
            HeapStr::Slice { len_ascii, .. } => (len_ascii.get() >> 30) as u8,
        }
    }

    /// Whether the string is pure ASCII, computing and caching the answer on
    /// first use. ASCII strings have char index == byte index, which turns
    /// char-indexed ops (`charCodeAt`, `substring`, `length`, ...) into O(1)
    /// byte addressing.
    #[inline]
    pub fn is_ascii_cached(&self) -> bool {
        match self.ascii_state() {
            ascii_flag::YES => true,
            ascii_flag::NO => false,
            _ => {
                let a = self.as_str().is_ascii();
                self.set_ascii_state(if a { ascii_flag::YES } else { ascii_flag::NO });
                a
            }
        }
    }

    #[inline]
    fn set_ascii_state(&self, state: u8) {
        match self {
            HeapStr::Shared(_, ascii) => ascii.set(state),
            HeapStr::Ext { ascii, .. } => ascii.set(state),
            HeapStr::Slice { len_ascii, .. } => {
                let len = len_ascii.get() & SLICE_LEN_MASK;
                len_ascii.set(len | ((state as u32) << 30));
            }
        }
    }

    /// True when this view ends exactly at the buffer tip, i.e. appending to
    /// the buffer extends this string without disturbing any other view.
    #[inline]
    pub fn is_tip(&self) -> bool {
        match self {
            HeapStr::Ext { buf, len, .. } => unsafe { (&*buf.get()).len() == *len },
            _ => false,
        }
    }
}

impl std::fmt::Debug for HeapStr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("HeapStr").field(&self.as_str()).finish()
    }
}

impl std::fmt::Display for HeapStr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq for HeapStr {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for HeapStr {}

impl std::ops::Deref for HeapStr {
    type Target = str;
    #[inline]
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for HeapStr {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<RuntimeString> for HeapStr {
    fn from(s: RuntimeString) -> Self {
        HeapStr::shared(s)
    }
}

// `repr(u8)` pins the discriminant to the first byte with a defined layout
// (RFC 2195), so JIT code can type-check a heap slot with one byte load.
#[derive(Debug, Clone)]
#[repr(u8)]
pub enum HeapObj {
    Str(HeapStr),
    Array(VmArray),
    Object(ObjRef),

    Module(Rc<ModuleObj>),

    FrozenModule(Arc<FrozenModuleObj>),
    VmClosure(Rc<VmClosure>),
    Class(Rc<ClassObj>),
    NativeFn(&'static str, NativeFn),
    BoundMethod(Box<BoundMethod>),
    Map(MapRef),
    Set(SetRef),
    Task(Rc<LazyTask>),
    TaskHandle(AsyncTask),
    Range(RangeData),
    Symbol(RuntimeSymbol),
    EnumVariant(Box<EnumVariantData>),
    BigInt(i128),
    Decimal(Box<rust_decimal::Decimal>),
    Char(char),
    Generator(GeneratorObj),
    AsyncQueue(AsyncQueue),
    Spread(VmValue),
    VmValue(Box<dyn VmValuePayload>),
}

impl HeapObj {
    /// The single canonical [`TypeTag`] of this heap object. Callables
    /// (closure / native fn / bound method) coalesce to `Function`; modules
    /// present as `Object`; spreads as `Array`; opaque host payloads as `VmRef`.
    /// All value-kind name rendering flows through this — see [`TypeTag::name`].
    pub fn tag(&self) -> varn_base::TypeTag {
        use varn_base::TypeTag;
        match self {
            HeapObj::Str(_) => TypeTag::Str,
            HeapObj::Array(_) => TypeTag::Array,
            HeapObj::Object(_) | HeapObj::Module(_) | HeapObj::FrozenModule(_) => TypeTag::Object,
            HeapObj::VmClosure(_) | HeapObj::NativeFn(..) | HeapObj::BoundMethod(_) => {
                TypeTag::Function
            }
            HeapObj::Class(_) => TypeTag::Class,
            HeapObj::Map(_) => TypeTag::Map,
            HeapObj::Set(_) => TypeTag::Set,
            HeapObj::Task(_) => TypeTag::Task,
            HeapObj::TaskHandle(_) => TypeTag::TaskHandle,
            HeapObj::Range(_) => TypeTag::Range,
            HeapObj::Symbol(_) => TypeTag::Symbol,
            HeapObj::EnumVariant(_) => TypeTag::Enum,
            HeapObj::BigInt(_) => TypeTag::BigInt,
            HeapObj::Decimal(_) => TypeTag::Decimal,
            HeapObj::Char(_) => TypeTag::Char,
            HeapObj::Generator(_) => TypeTag::Generator,
            HeapObj::AsyncQueue(_) => TypeTag::AsyncQueue,
            HeapObj::Spread(_) => TypeTag::Array,
            HeapObj::VmValue(_) => TypeTag::VmRef,
        }
    }
}

#[derive(Clone)]
pub struct HeapInner {
    /// Identity of this object table, and therefore of every `VmValue` handle
    /// into it. Compiled code bakes those handles as immediates, so it is only
    /// valid while this heap is: see `FunctionProto::jit_epoch`. Shared by
    /// `Heap::clone` (a nested context reaches the same objects) and fresh for
    /// `deep_clone` (equal contents, separate table).
    pub jit_epoch: u64,
    pub alloc_count: u64,
    pub intrinsic_classes: FxHashMap<String, Rc<ClassObj>>,
    pub gc_collections: u64,
    pub gc_total_freed: u64,
    pub gc_alloc_since_collect: u64,
    pub nursery: Nursery,
    free: Vec<u32>,
    objects: Vec<Option<HeapObj>>,
    string_interner: FxHashMap<RuntimeString, u32>,
    symbol_interner: FxHashMap<RuntimeSymbol, u32>,
    array_interner: FxHashMap<ArrayRef, u32>,
    object_interner: FxHashMap<ObjRef, u32>,
    map_interner: FxHashMap<MapRef, u32>,
    set_interner: FxHashMap<SetRef, u32>,
    bigint_interner: FxHashMap<i128, u32>,
    decimal_interner: FxHashMap<rust_decimal::Decimal, u32>,
    char_interner: FxHashMap<char, u32>,
    gc_collector: Option<GcCollector>,
    pub hotspot: Option<Rc<RefCell<HotspotCounters>>>,
    /// Raw old-gen indices of objects that hold Rust-side `Value`s the
    /// remembered set cannot track (VmClosure, BoundMethod, Class, Module).
    /// The minor GC scans only these instead of walking the whole old gen;
    /// rebuilt after every major GC sweep to drop stale entries.
    scan_roots: Vec<u32>,
    /// Identity pointer → packed old-gen index, for heap objects that
    /// Rust-side `Value`s reference by identity instead of through a content
    /// interner (Class, Task, TaskHandle, Generator, AsyncQueue, VmClosure).
    /// Replaces the full-heap scans `value_heap_idx` used to do. Entries
    /// cannot go stale between sweeps: the heap slot itself owns a clone of
    /// the Rc/task, so the pointer is never reused while the slot is live,
    /// and `compact_interners` purges dead slots right after every sweep.
    identity_index: FxHashMap<usize, u32>,
}

#[inline]
fn alloc_into(
    objects: &mut Vec<Option<HeapObj>>,
    free: &mut Vec<u32>,
    alloc_count: &mut u64,
    gc_alloc_since_collect: &mut u64,
    obj: HeapObj,
) -> u32 {
    *alloc_count += 1;
    *gc_alloc_since_collect += 1;
    if let Some(idx) = free.pop() {
        objects[idx as usize] = Some(obj);
        idx
    } else {
        let idx = objects.len() as u32;
        objects.push(Some(obj));
        idx
    }
}

impl Drop for HeapInner {
    fn drop(&mut self) {
        // Last owner of this object table. Every compiled entry that baked a
        // handle into it is now code for a heap that no longer exists — and the
        // protos holding those entries outlive this drop, so they must be
        // stripped here rather than left for the next run to enter.
        crate::clif_link::invalidate_epoch(self.jit_epoch);
    }
}

impl HeapInner {
    pub fn new() -> Self {
        Self {
            jit_epoch: crate::clif_link::next_epoch(),
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

    /// Stable identity pointer for heap objects resolvable only by identity
    /// (see `identity_index`). Returns `None` for everything else.
    #[inline]
    fn identity_key(obj: &HeapObj) -> Option<usize> {
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

    /// Whether the minor GC must scan this old-gen object for untracked
    /// nursery references (see `scan_roots`). Generators qualify because a
    /// suspended body writes fresh nursery indices into its saved stack
    /// between collections and no write barrier covers those Rust-side Vecs.
    #[inline(always)]
    fn needs_minor_scan(obj: &HeapObj) -> bool {
        matches!(
            obj,
            HeapObj::VmClosure(_)
                | HeapObj::BoundMethod(_)
                | HeapObj::Class(_)
                | HeapObj::Module(_)
                | HeapObj::Generator(_)
        )
    }

    pub fn scan_roots(&self) -> &[u32] {
        &self.scan_roots
    }

    /// Rebuild `scan_roots` from the live old gen (after a major GC sweep
    /// invalidated indices).
    pub fn rebuild_scan_roots(&mut self) {
        self.scan_roots.clear();
        for (idx, obj) in self.objects.iter().enumerate() {
            if let Some(obj) = obj {
                if Self::needs_minor_scan(obj) {
                    self.scan_roots.push(idx as u32);
                }
            }
        }
    }

    /// Check if a VmValue represents an integer. `int` is a pure inline i48;
    /// there is no boxed integer form (out-of-range values wrap, per
    /// `varn_core::numeric`).
    #[inline(always)]
    pub fn is_int(&self, v: VmValue) -> bool {
        v.is_int()
    }

    /// Extract the i64 value from an inline integer.
    #[inline(always)]
    pub fn as_int(&self, v: VmValue) -> i64 {
        if v.is_int() {
            v.as_int()
        } else {
            0
        }
    }

    /// Convert a VmValue to f64 (handles inline int and f64).
    #[inline(always)]
    pub fn to_f64_val(&self, v: VmValue) -> f64 {
        if v.is_f64() {
            v.as_f64()
        } else if v.is_int() {
            v.as_int() as f64
        } else {
            0.0
        }
    }

    pub fn set_intrinsic_class(&mut self, name: &str, cls: Rc<ClassObj>) {
        self.intrinsic_classes.insert(name.to_string(), cls);
    }

    pub fn get_intrinsic_class(&self, name: &str) -> Option<Rc<ClassObj>> {
        self.intrinsic_classes.get(name).cloned()
    }

    #[inline]
    pub fn alloc_native_fn(&mut self, f: NativeFn, name: &'static str) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::NativeFn(name, f)))
    }

    pub fn alloc(&mut self, obj: HeapObj) -> u32 {
        if let Some(ref h) = self.hotspot.clone() {
            h.borrow_mut().record_alloc(obj.tag().name());
        }
        let mut obj = obj;
        match obj {
            HeapObj::Str(_)
            | HeapObj::Array(_)
            | HeapObj::Object(_)
            | HeapObj::VmClosure(_)
            | HeapObj::BoundMethod(_)
            | HeapObj::EnumVariant(_)
            | HeapObj::Range(_)
            | HeapObj::Char(_)
            | HeapObj::Symbol(_)
            | HeapObj::BigInt(_)
            | HeapObj::Decimal(_) => match self.nursery.try_alloc(obj) {
                Ok(ni) => return ni,
                Err(returned_obj) => {
                    obj = returned_obj;
                }
            },
            _ => {}
        }
        let track = Self::needs_minor_scan(&obj);
        let identity = Self::identity_key(&obj);
        let raw = alloc_into(
            &mut self.objects,
            &mut self.free,
            &mut self.alloc_count,
            &mut self.gc_alloc_since_collect,
            obj,
        );
        if track {
            self.scan_roots.push(raw);
        }
        if let Some(key) = identity {
            self.identity_index.insert(key, pack_old_idx(raw));
        }
        pack_old_idx(raw)
    }

    pub fn alloc_raw(&mut self, obj: HeapObj) -> u32 {
        self.alloc_count += 1;
        let track = Self::needs_minor_scan(&obj);
        let identity = Self::identity_key(&obj);
        let idx = if let Some(idx) = self.free.pop() {
            self.objects[idx as usize] = Some(obj);
            idx
        } else {
            let idx = self.objects.len() as u32;
            self.objects.push(Some(obj));
            idx
        };
        if track {
            self.scan_roots.push(idx);
        }
        if let Some(key) = identity {
            self.identity_index.insert(key, pack_old_idx(idx));
        }
        idx
    }

    #[inline(always)]
    pub fn needs_minor_gc(&self) -> bool {
        self.nursery.is_full()
    }

    #[inline(always)]
    pub fn write_barrier(&mut self, parent_packed_idx: u32, new_val: VmValue) {
        if is_old_idx(parent_packed_idx)
            && new_val.is_heap()
            && is_nursery_idx(new_val.as_heap_idx())
        {
            self.nursery.remember(parent_packed_idx);
        }
    }

    #[inline(always)]
    pub fn get_by_idx(&self, idx: u32) -> Option<&HeapObj> {
        if is_nursery_idx(idx) {
            self.nursery.get(idx)
        } else {
            self.objects.get(old_idx_raw(idx) as usize)?.as_ref()
        }
    }

    #[inline(always)]
    pub fn get_by_idx_mut(&mut self, idx: u32) -> Option<&mut HeapObj> {
        if is_nursery_idx(idx) {
            self.nursery.get_mut(idx)
        } else {
            self.objects.get_mut(old_idx_raw(idx) as usize)?.as_mut()
        }
    }

    pub fn minor_gc(&mut self, stack: &mut [VmValue], extra_packed: &[u32]) {
        let mut nursery = std::mem::take(&mut self.nursery);
        nursery.collect(self, stack, extra_packed);
        self.nursery = nursery;
    }

    #[inline(always)]
    pub fn get(&self, idx: u32) -> Option<&HeapObj> {
        self.get_by_idx(idx)
    }

    #[inline(always)]
    pub fn get_closure(&self, idx: u32) -> Option<&VmClosure> {
        let obj = if is_nursery_idx(idx) {
            self.nursery.get(idx)?
        } else {
            self.objects.get(old_idx_raw(idx) as usize)?.as_ref()?
        };
        match obj {
            HeapObj::VmClosure(c) => Some(&**c),
            _ => None,
        }
    }

    #[inline(always)]
    pub fn get_or_panic(&self, idx: u32) -> &HeapObj {
        self.get_by_idx(idx).expect("invalid heap index")
    }

    #[inline(always)]
    pub fn get_mut(&mut self, idx: u32) -> Option<&mut HeapObj> {
        self.get_by_idx_mut(idx)
    }

    #[inline(always)]
    pub unsafe fn get_mut_unchecked(&mut self, idx: u32) -> &mut HeapObj {
        self.get_by_idx_mut(idx).expect("invalid heap index")
    }

    #[inline(always)]
    pub fn get_raw(&self, raw_old_idx: u32) -> Option<&HeapObj> {
        self.objects.get(raw_old_idx as usize)?.as_ref()
    }

    #[inline(always)]
    pub fn get_raw_mut(&mut self, raw_old_idx: u32) -> Option<&mut HeapObj> {
        self.objects.get_mut(raw_old_idx as usize)?.as_mut()
    }

    pub fn intern(&mut self, val: Value) -> VmValue {
        match val {
            Value::Null => VmValue::null(),
            Value::Bool(b) => VmValue::from_bool(b),
            // `int` is i48: host-supplied values outside the range wrap, the
            // same rule every tier applies (varn_core::numeric).
            Value::Int(n) => VmValue::from_int(n),
            Value::Float(f) => VmValue::from_f64(f),
            Value::Char(c) => {
                let packed = match self.char_interner.entry(c) {
                    Entry::Occupied(e) => *e.get(),
                    Entry::Vacant(e) => *e.insert(pack_old_idx(alloc_into(
                        &mut self.objects,
                        &mut self.free,
                        &mut self.alloc_count,
                        &mut self.gc_alloc_since_collect,
                        HeapObj::Char(c),
                    ))),
                };
                VmValue::from_heap_idx(packed)
            }
            Value::Str(s) => self.alloc_str(s),
            Value::BigInt(n) => {
                let n_val = *n;
                let packed = match self.bigint_interner.entry(n_val) {
                    Entry::Occupied(e) => *e.get(),
                    Entry::Vacant(e) => *e.insert(pack_old_idx(alloc_into(
                        &mut self.objects,
                        &mut self.free,
                        &mut self.alloc_count,
                        &mut self.gc_alloc_since_collect,
                        HeapObj::BigInt(n_val),
                    ))),
                };
                VmValue::from_heap_idx(packed)
            }
            Value::Decimal(d) => {
                let d_val = *d;
                let packed = match self.decimal_interner.entry(d_val) {
                    Entry::Occupied(e) => *e.get(),
                    Entry::Vacant(e) => *e.insert(pack_old_idx(alloc_into(
                        &mut self.objects,
                        &mut self.free,
                        &mut self.alloc_count,
                        &mut self.gc_alloc_since_collect,
                        HeapObj::Decimal(Box::new(d_val)),
                    ))),
                };
                VmValue::from_heap_idx(packed)
            }
            Value::Array(a) => {
                let guard = a.borrow();
                let vm_items: Vec<VmValue> = guard.iter().map(|v| self.intern(v.clone())).collect();
                let va = VmArray::from_items(vm_items);
                let idx = self.alloc(HeapObj::Array(va));
                VmValue::from_heap_idx(idx)
            }
            Value::Object(o) => {
                if let Some(&idx) = self.object_interner.get(&o) {
                    VmValue::from_heap_idx(idx)
                } else {
                    let idx = self.alloc(HeapObj::Object(o.clone()));
                    self.object_interner.insert(o, idx);
                    VmValue::from_heap_idx(idx)
                }
            }
            Value::Class(c) => VmValue::from_heap_idx(self.alloc(HeapObj::Class(c))),
            Value::NativeFn(b) => VmValue::from_heap_idx(self.alloc(HeapObj::NativeFn(b.1, b.0))),
            Value::BoundMethod(b) => VmValue::from_heap_idx(self.alloc(HeapObj::BoundMethod(b))),
            Value::Map(m) => {
                if let Some(&idx) = self.map_interner.get(&m) {
                    VmValue::from_heap_idx(idx)
                } else {
                    let idx = self.alloc(HeapObj::Map(m.clone()));
                    self.map_interner.insert(m, idx);
                    VmValue::from_heap_idx(idx)
                }
            }
            Value::Set(s) => {
                if let Some(&idx) = self.set_interner.get(&s) {
                    VmValue::from_heap_idx(idx)
                } else {
                    let idx = self.alloc(HeapObj::Set(s.clone()));
                    self.set_interner.insert(s, idx);
                    VmValue::from_heap_idx(idx)
                }
            }
            Value::Task(t) => VmValue::from_heap_idx(self.alloc(HeapObj::Task(t))),
            Value::TaskHandle(t) => VmValue::from_heap_idx(self.alloc(HeapObj::TaskHandle(t))),
            Value::Range(r) => VmValue::from_heap_idx(self.alloc(HeapObj::Range(*r))),
            Value::Symbol(s) => self.alloc_symbol(s),
            Value::EnumVariant(e) => VmValue::from_heap_idx(self.alloc(HeapObj::EnumVariant(e))),
            Value::Spread(v) => {
                let inner = self.intern(*v);
                VmValue::from_heap_idx(self.alloc(HeapObj::Spread(inner)))
            }
            Value::Generator(g) => VmValue::from_heap_idx(self.alloc(HeapObj::Generator(g))),
            Value::AsyncQueue(q) => VmValue::from_heap_idx(self.alloc(HeapObj::AsyncQueue(q))),
            Value::VmValue(payload) => {
                if let Some(vref) = payload.as_any().downcast_ref::<VmValueRef>() {
                    vref.0
                } else if let Some(wrapper) = payload.as_any().downcast_ref::<VmClosurePayload>() {
                    VmValue::from_heap_idx(self.alloc(HeapObj::VmClosure(wrapper.0.clone())))
                } else {
                    VmValue::from_heap_idx(self.alloc(HeapObj::VmValue(payload)))
                }
            }
            Value::Module(m) => VmValue::from_heap_idx(self.alloc(HeapObj::Module(m))),
        }
    }

    #[track_caller]
    pub fn extract(&self, nv: VmValue) -> Value {
        match self.extract_val(nv) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "[GC DEBUG] heap.extract failed on nv=0x{:016x} (is_heap={}, heap_idx={})",
                    nv.0,
                    nv.is_heap(),
                    if nv.is_heap() {
                        nv.as_heap_idx() as i64
                    } else {
                        -1
                    }
                );
                eprintln!(
                    "[GC DEBUG] Caller location: {}",
                    std::panic::Location::caller()
                );
                panic!("heap.extract: dangling or corrupted heap reference: {e}");
            }
        }
    }

    pub fn extract_val(&self, nv: VmValue) -> VmResult<Value> {
        if nv.is_null() {
            return Ok(Value::Null);
        }
        if nv.is_bool() {
            return Ok(Value::Bool(nv.as_bool()));
        }
        if nv.is_int() {
            return Ok(Value::Int(nv.as_int()));
        }
        if nv.is_f64() {
            return Ok(Value::Float(nv.as_f64()));
        }
        if nv.is_sso() {
            let mut buf = [0u8; 5];
            let s = nv.sso_as_str(&mut buf);
            return Ok(Value::Str(Rc::from(s)));
        }
        if nv.is_heap() {
            let obj = self
                .get_by_idx(nv.as_heap_idx())
                .ok_or_else(|| RuntimeError::new("invalid heap ref"))?;
            return Ok(match obj {
                HeapObj::Str(s) => Value::Str(s.to_shared()),
                HeapObj::Array(a) => {
                    let val_items: Vec<Value> = (0..a.len())
                        .map(|i| self.extract(a.get_vm(i).unwrap()))
                        .collect();
                    Value::Array(varn_types::value::ArrayRef::new(val_items))
                }
                HeapObj::Object(o) => Value::Object(o.clone()),
                HeapObj::VmClosure(c) => Value::VmValue(Box::new(VmClosurePayload(c.clone()))),
                HeapObj::Class(c) => Value::Class(c.clone()),
                HeapObj::NativeFn(name, f) => Value::NativeFn(Box::new((*f, *name))),
                HeapObj::BoundMethod(m) => Value::BoundMethod(m.clone()),
                HeapObj::Map(m) => Value::Map(m.clone()),
                HeapObj::Set(s) => Value::Set(s.clone()),
                HeapObj::Task(t) => Value::Task(t.clone()),
                HeapObj::TaskHandle(t) => Value::TaskHandle(t.clone()),
                HeapObj::Range(r) => Value::Range(Box::new(r.clone())),
                HeapObj::Symbol(s) => Value::Symbol(s.clone()),
                HeapObj::EnumVariant(data) => Value::EnumVariant(data.clone()),
                HeapObj::BigInt(n) => Value::BigInt(Box::new(*n)),
                HeapObj::Decimal(d) => Value::Decimal(d.clone()),
                HeapObj::Char(c) => Value::Char(*c),
                HeapObj::Generator(g) => Value::Generator(g.clone()),
                HeapObj::AsyncQueue(q) => Value::AsyncQueue(q.clone()),
                HeapObj::Spread(inner) => Value::Spread(Box::new(self.extract(*inner))),
                HeapObj::VmValue(payload) => Value::VmValue(payload.clone_payload()),
                HeapObj::Module(m) => Value::Module(m.clone()),

                HeapObj::FrozenModule(_) => Value::Null,
            });
        }
        Ok(Value::Null)
    }

    pub fn alloc_str(&mut self, s: impl AsRef<str>) -> VmValue {
        let s_ref = s.as_ref();

        if let Some(sso) = VmValue::try_from_sso(s_ref) {
            return sso;
        }

        let rs: RuntimeString = Rc::from(s_ref);

        if let Some(&packed) = self.string_interner.get(&rs) {
            let raw = old_idx_raw(packed);
            if self
                .objects
                .get(raw as usize)
                .map(|o| o.is_some())
                .unwrap_or(false)
            {
                return VmValue::from_heap_idx(packed);
            }
            self.string_interner.remove(&rs);
        }

        let idx = match self
            .nursery
            .try_alloc(HeapObj::Str(HeapStr::shared(rs.clone())))
        {
            Ok(ni) => ni,
            Err(obj) => {
                let oi = alloc_into(
                    &mut self.objects,
                    &mut self.free,
                    &mut self.alloc_count,
                    &mut self.gc_alloc_since_collect,
                    obj,
                );
                let packed = pack_old_idx(oi);
                self.string_interner.insert(rs, packed);
                packed
            }
        };
        VmValue::from_heap_idx(idx)
    }

    pub fn alloc_str_interned(&mut self, s: impl AsRef<str>) -> VmValue {
        let s_ref = s.as_ref();
        if let Some(sso) = VmValue::try_from_sso(s_ref) {
            return sso;
        }
        // Borrowed lookup first: the hot path (already-interned key) must
        // not allocate an Rc<str> just to probe the map.
        if let Some(&packed) = self.string_interner.get(s_ref) {
            let raw = old_idx_raw(packed);
            if self
                .objects
                .get(raw as usize)
                .map(|o| o.is_some())
                .unwrap_or(false)
            {
                return VmValue::from_heap_idx(packed);
            }
            self.string_interner.remove(s_ref);
        }
        let rs: RuntimeString = Rc::from(s_ref);
        let oi = alloc_into(
            &mut self.objects,
            &mut self.free,
            &mut self.alloc_count,
            &mut self.gc_alloc_since_collect,
            HeapObj::Str(HeapStr::shared(rs.clone())),
        );
        let packed = pack_old_idx(oi);
        self.string_interner.insert(rs, packed);
        VmValue::from_heap_idx(packed)
    }

    /// Allocate a string that is *not* registered in the content interner.
    ///
    /// Used for dynamically produced strings (concatenation, slicing, coercion).
    /// Interning these would hash the full contents on every allocation — turning
    /// an accumulation loop into O(n^2) work — and would retain a reference to
    /// every transient string, defeating the GC and exhausting memory. Only
    /// known-constant strings should ever enter the interner (`alloc_str_interned`).
    pub fn alloc_str_dynamic(&mut self, s: impl AsRef<str>) -> VmValue {
        let s_ref = s.as_ref();

        if let Some(sso) = VmValue::try_from_sso(s_ref) {
            return sso;
        }

        let rs: RuntimeString = Rc::from(s_ref);
        let idx = match self.nursery.try_alloc(HeapObj::Str(HeapStr::shared(rs))) {
            Ok(ni) => ni,
            Err(obj) => pack_old_idx(alloc_into(
                &mut self.objects,
                &mut self.free,
                &mut self.alloc_count,
                &mut self.gc_alloc_since_collect,
                obj,
            )),
        };
        VmValue::from_heap_idx(idx)
    }

    /// Allocate the substring `handle.as_str()[bs..be]` (byte offsets on char
    /// boundaries). Short results become SSO values; longer results over an
    /// immutable buffer become zero-copy `Slice` views (composing when the
    /// source is itself a `Slice`); `Ext` sources copy, since their backing
    /// buffer can reallocate on growth.
    pub fn alloc_substring(&mut self, handle: &HeapStr, bs: usize, be: usize) -> VmValue {
        let sub = &handle.as_str()[bs..be];
        if let Some(sso) = VmValue::try_from_sso(sub) {
            return sso;
        }
        // A substring of an ASCII string is ASCII; anything else is resolved
        // lazily on first query.
        let flag = if handle.is_ascii_cached() {
            ascii_flag::YES
        } else {
            ascii_flag::UNKNOWN
        };
        let len = be - bs;
        if len as u64 <= SLICE_LEN_MASK as u64 {
            match handle {
                HeapStr::Shared(rc, _) => {
                    let hs = HeapStr::slice_of(Rc::clone(rc), bs, len, flag);
                    return self.alloc_str_view(hs);
                }
                HeapStr::Slice { src, off, .. } => {
                    let hs = HeapStr::slice_of(Rc::clone(src), *off as usize + bs, len, flag);
                    return self.alloc_str_view(hs);
                }
                HeapStr::Ext { .. } => {}
            }
        }
        self.alloc_str_dynamic(sub)
    }

    /// Allocate a heap slot for an arbitrary string payload (typically an
    /// extensible-buffer view from `str_concat`). Never interned, never SSO.
    pub fn alloc_str_view(&mut self, hs: HeapStr) -> VmValue {
        let idx = match self.nursery.try_alloc(HeapObj::Str(hs)) {
            Ok(ni) => ni,
            Err(obj) => pack_old_idx(alloc_into(
                &mut self.objects,
                &mut self.free,
                &mut self.alloc_count,
                &mut self.gc_alloc_since_collect,
                obj,
            )),
        };
        VmValue::from_heap_idx(idx)
    }

    pub fn alloc_symbol(&mut self, s: RuntimeSymbol) -> VmValue {
        let packed = match self.symbol_interner.entry(s) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(e) => {
                let packed = pack_old_idx(alloc_into(
                    &mut self.objects,
                    &mut self.free,
                    &mut self.alloc_count,
                    &mut self.gc_alloc_since_collect,
                    HeapObj::Symbol(e.key().clone()),
                ));
                *e.insert(packed)
            }
        };
        VmValue::from_heap_idx(packed)
    }

    /// The single allocation point for arrays built from boxed values —
    /// literals (interpreter and both JIT tiers), natives, spreads. The repr
    /// is chosen from the values here (see [`VmArray::from_items`]) so every
    /// producer gets a raw buffer for free when the elements admit one.
    pub fn alloc_array_vm(&mut self, items: Vec<VmValue>) -> VmValue {
        let va = VmArray::from_items(items);
        VmValue::from_heap_idx(self.alloc(HeapObj::Array(va)))
    }

    pub fn alloc_array(&mut self, items: Vec<Value>) -> VmValue {
        let vm_items: Vec<VmValue> = items.into_iter().map(|v| self.intern(v)).collect();
        self.alloc_array_vm(vm_items)
    }

    pub fn alloc_object(&mut self) -> VmValue {
        let oref = ObjRef::empty();
        VmValue::from_heap_idx(self.alloc(HeapObj::Object(oref)))
    }

    pub fn alloc_object_with_shape(
        &mut self,
        shape: &Rc<varn_types::Shape>,
        values: Vec<VmValue>,
    ) -> VmValue {
        let oref = ObjRef::with_shape(Rc::clone(shape), values);
        VmValue::from_heap_idx(self.alloc(HeapObj::Object(oref)))
    }
    
    pub fn make_int(&mut self, n: i64) -> VmValue {
        VmValue::from_int(n)
    }

    /// Read-only canonical map key for a borrowed string: `None` means the
    /// string was never interned, so no map can contain it as a key
    /// (insertion canonicalizes through the interner).
    pub fn lookup_str_map_key(&self, s: &str) -> Option<varn_types::value::MapKey> {
        if let Some(sso) = VmValue::try_from_sso(s) {
            return Some(varn_types::value::MapKey(sso));
        }
        self.string_interner
            .get(s)
            .map(|&packed| varn_types::value::MapKey(VmValue::from_heap_idx(packed)))
    }

    /// Read-only counterpart of [`Self::canonical_map_key`] for lookups:
    /// `None` means "definitely not a key of any map" (its canonical form
    /// was never interned), so `get`/`has`/`delete` need no allocation.
    pub fn lookup_map_key(&self, v: VmValue) -> Option<varn_types::value::MapKey> {
        use varn_types::value::MapKey;
        if v.is_f64() {
            if v.as_f64() == 0.0 {
                return Some(MapKey(VmValue::from_f64(0.0)));
            }
            return Some(MapKey(v));
        }
        if !v.is_heap() {
            return Some(MapKey(v));
        }
        match self.get_by_idx(v.as_heap_idx()) {
            Some(HeapObj::Str(s)) => self.lookup_str_map_key(s.as_str()),
            Some(HeapObj::Char(c)) => self
                .char_interner
                .get(c)
                .map(|&p| MapKey(VmValue::from_heap_idx(p))),
            Some(HeapObj::BigInt(b)) => self
                .bigint_interner
                .get(b)
                .map(|&p| MapKey(VmValue::from_heap_idx(p))),
            Some(HeapObj::Decimal(d)) => self
                .decimal_interner
                .get(d)
                .map(|&p| MapKey(VmValue::from_heap_idx(p))),
            _ => Some(MapKey(v)),
        }
    }

    /// Canonicalize a value for `Map`/`Set` keying: content-equal keys must
    /// be bit-equal (see `varn_types::value::MapKey`). SSO/int/bool/null/
    /// symbol are canonical by representation; heap strings, chars,
    /// decimals and bigints canonicalize through the content interners
    /// (old-gen allocations, so minor GC never moves a canonical key);
    /// floats normalize `-0.0` (SameValueZero); everything else keys by
    /// identity.
    pub fn canonical_map_key(&mut self, v: VmValue) -> varn_types::value::MapKey {
        use varn_types::value::MapKey;
        if v.is_f64() {
            if v.as_f64() == 0.0 {
                return MapKey(VmValue::from_f64(0.0));
            }
            return MapKey(v);
        }
        if !v.is_heap() {
            return MapKey(v);
        }
        enum Canon {
            Str(String),
            Char(char),
            BigInt(i128),
            Decimal(rust_decimal::Decimal),
            Identity,
        }
        let canon = match self.get_by_idx(v.as_heap_idx()) {
            Some(HeapObj::Str(s)) => Canon::Str(s.as_str().to_owned()),
            Some(HeapObj::Char(c)) => Canon::Char(*c),
            Some(HeapObj::BigInt(b)) => Canon::BigInt(*b),
            Some(HeapObj::Decimal(d)) => Canon::Decimal(**d),
            _ => Canon::Identity,
        };
        match canon {
            Canon::Str(s) => MapKey(self.alloc_str_interned(s)),
            Canon::Char(c) => MapKey(self.intern(Value::Char(c))),
            Canon::BigInt(b) => MapKey(self.intern(Value::BigInt(Box::new(b)))),
            Canon::Decimal(d) => MapKey(self.intern(Value::Decimal(Box::new(d)))),
            Canon::Identity => MapKey(v),
        }
    }

    pub fn alloc_range(&mut self, start: i64, end: i64, inclusive: bool) -> VmValue {
        let r = RangeData {
            start,
            end,
            inclusive,
            step: 1,
        };
        VmValue::from_heap_idx(self.alloc(HeapObj::Range(r)))
    }

    pub fn alloc_decimal(&mut self, d: rust_decimal::Decimal) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::Decimal(Box::new(d))))
    }

    pub fn alloc_vm_closure(&mut self, c: Rc<VmClosure>) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::VmClosure(c)))
    }

    pub fn alloc_module(&mut self, m: Rc<ModuleObj>) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::Module(m)))
    }

    pub fn alloc_frozen_module(&mut self, m: Arc<FrozenModuleObj>) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::FrozenModule(m)))
    }

    pub fn str_val(&self, nv: VmValue) -> Option<RuntimeString> {
        if nv.is_sso() {
            let mut buf = [0u8; 5];
            let s = nv.sso_as_str(&mut buf);
            return Some(Rc::from(s));
        }
        if !nv.is_heap() {
            return None;
        }
        if let Some(HeapObj::Str(s)) = self.get_by_idx(nv.as_heap_idx()) {
            return Some(s.to_shared());
        }
        None
    }

    pub fn is_string(&self, nv: VmValue) -> bool {
        if nv.is_sso() {
            return true;
        }
        if nv.is_heap() {
            return matches!(self.get_by_idx(nv.as_heap_idx()), Some(HeapObj::Str(_)));
        }
        false
    }

    pub fn str_owned(&self, nv: VmValue) -> Option<String> {
        if nv.is_sso() {
            let mut buf = [0u8; 5];
            let s = nv.sso_as_str(&mut buf);
            return Some(s.to_owned());
        }
        if nv.is_heap() {
            if let Some(HeapObj::Str(s)) = self.get_by_idx(nv.as_heap_idx()) {
                return Some(s.to_string());
            }
        }
        None
    }

    pub fn str_repr_borrowed<'a>(&'a self, nv: VmValue) -> std::borrow::Cow<'a, str> {
        if nv.is_heap() {
            if let Some(HeapObj::Str(s)) = self.get_by_idx(nv.as_heap_idx()) {
                return std::borrow::Cow::Borrowed(s.as_ref());
            }
        }
        std::borrow::Cow::Owned(self.str_repr(nv))
    }

    /// Append a value's string form directly to `out`, allocating nothing of
    /// its own for the common leaf cases (strings, ints, bools, null). Used
    /// by `BuildStr` so concatenating template parts costs one output
    /// allocation rather than one `String` per part.
    /// Renders `nv` into any string sink. Generic so a concat can render
    /// straight into a stack buffer ([`crate::strbuf::StrBuf`]) instead of a
    /// heap-allocated `String`; integers go through [`crate::strbuf::itoa`]
    /// rather than `core::fmt`, which dominated integer concatenation.
    pub fn str_repr_into<W: std::fmt::Write>(&self, nv: VmValue, out: &mut W) {
        use crate::strbuf::{itoa, INT_MAX_DIGITS};
        if nv.is_null() {
            let _ = out.write_str("null");
        } else if nv.is_bool() {
            let _ = out.write_str(if nv.as_bool() { "true" } else { "false" });
        } else if nv.is_int() {
            let mut buf = [0u8; INT_MAX_DIGITS];
            let _ = out.write_str(itoa(nv.as_int(), &mut buf));
        } else if nv.is_f64() {
            let f = nv.as_f64();
            if f.fract() == 0.0 && f.abs() < 1e15 {
                let mut buf = [0u8; INT_MAX_DIGITS];
                let _ = out.write_str(itoa(f as i64, &mut buf));
            } else {
                let _ = write!(out, "{}", f);
            }
        } else if nv.is_sso() {
            let mut buf = [0u8; 5];
            let _ = out.write_str(nv.sso_as_str(&mut buf));
        } else if nv.is_heap() {
            if let Some(HeapObj::Str(s)) = self.get_by_idx(nv.as_heap_idx()) {
                let _ = out.write_str(s.as_ref());
                return;
            }
            let _ = out.write_str(&self.str_repr(nv));
        } else {
            let _ = out.write_str(&self.str_repr(nv));
        }
    }

    pub fn str_repr(&self, nv: VmValue) -> String {
        if nv.is_null() {
            return "null".into();
        }
        if nv.is_bool() {
            return nv.as_bool().to_string();
        }
        if nv.is_int() {
            return nv.as_int().to_string();
        }
        if nv.is_f64() {
            let f = nv.as_f64();
            if f.fract() == 0.0 && f.abs() < 1e15 {
                return format!("{}", f as i64);
            }
            return format!("{}", f);
        }
        if nv.is_sso() {
            let mut buf = [0u8; 5];
            return nv.sso_as_str(&mut buf).to_owned();
        }
        if nv.is_heap() {
            return match self.get_by_idx(nv.as_heap_idx()) {
                Some(HeapObj::Str(s)) => s.to_string(),
                Some(HeapObj::Char(c)) => c.to_string(),
                Some(HeapObj::Array(a)) => {
                    let parts: Vec<_> = (0..a.len())
                        .map(|i| self.str_repr(a.get_vm(i).unwrap()))
                        .collect();
                    format!("[{}]", parts.join(", "))
                }
                Some(HeapObj::Object(_)) => "[object Object]".into(),
                Some(HeapObj::VmClosure(nc)) => format!(
                    "[Function {}]",
                    nc.proto.name.as_deref().unwrap_or("<anon>")
                ),
                Some(HeapObj::NativeFn(name, _)) => format!("[NativeFn: {}]", name),
                Some(HeapObj::BoundMethod(method)) => match &method.target {
                    varn_types::value::BoundMethodTarget::Native { name, .. } => {
                        format!("[Function {}]", name)
                    }
                    varn_types::value::BoundMethodTarget::Vm { .. } => "[BoundMethod]".into(),
                },
                Some(HeapObj::Class(c)) => format!("[class {}]", c.name),
                Some(HeapObj::BigInt(n)) => n.to_string(),
                Some(HeapObj::Decimal(d)) => d.to_string(),
                _ => "[object]".into(),
            };
        }
        "null".into()
    }

    pub fn objects_len(&self) -> u32 {
        self.objects.len() as u32
    }

    pub fn get_heap_idx(&self, val: VmValue) -> Option<u32> {
        if val.is_heap() {
            Some(val.as_heap_idx())
        } else {
            None
        }
    }

    pub fn free_list_mut(&mut self) -> &mut Vec<u32> {
        &mut self.free
    }

    pub fn should_trigger_gc(&self) -> bool {
        let used = self.objects.len() - self.free.len();

        self.free.len() * 5 < self.objects.len() || used > 10000
    }

    #[inline(always)]
    pub fn needs_gc(&self) -> bool {
        let live_count = (self.objects.len() - self.free.len()) as u64;
        self.gc_alloc_since_collect >= 16384.max(live_count)
    }

    pub fn compact_interners(&mut self) {
        self.string_interner.retain(|_, &mut packed| {
            let raw = old_idx_raw(packed);
            self.objects
                .get(raw as usize)
                .map(|o| o.is_some())
                .unwrap_or(false)
        });

        let check = |packed: u32, objects: &Vec<Option<HeapObj>>| -> bool {
            let raw = old_idx_raw(packed);
            objects
                .get(raw as usize)
                .map(|o| o.is_some())
                .unwrap_or(false)
        };

        self.symbol_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.char_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.bigint_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.decimal_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.array_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.object_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.map_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.set_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.identity_index
            .retain(|_, &mut packed| check(packed, &self.objects));
    }

    pub fn objects(&self) -> &Vec<Option<HeapObj>> {
        &self.objects
    }

    pub fn identity_index(&self) -> &FxHashMap<usize, u32> {
        &self.identity_index
    }

    /// Resolves a `Value` to its packed heap index. All arms return PACKED
    /// indices (old-gen bit set) — returning a raw old-gen index here made
    /// the minor GC misread it as a nursery index and stamp an unrelated
    /// evacuated object over class vtables/statics (second-class-ctor
    /// corruption, 2026-07-09).
    pub fn value_heap_idx(&self, val: &varn_types::Value) -> Option<u32> {
        match val {
            varn_types::Value::Str(s) => self.string_interner.get(s).copied(),
            varn_types::Value::Array(a) => self.array_interner.get(a).copied(),
            varn_types::Value::Object(o) => self.object_interner.get(o).copied(),
            varn_types::Value::Map(m) => self.map_interner.get(m).copied(),
            varn_types::Value::Set(s) => self.set_interner.get(s).copied(),
            varn_types::Value::BigInt(b) => self.bigint_interner.get(b.as_ref()).copied(),
            varn_types::Value::Decimal(d) => self.decimal_interner.get(d.as_ref()).copied(),
            varn_types::Value::Char(c) => self.char_interner.get(c).copied(),
            varn_types::Value::Class(c) => self
                .identity_index
                .get(&(Rc::as_ptr(c) as usize))
                .copied(),
            varn_types::Value::Task(t) => self
                .identity_index
                .get(&(Rc::as_ptr(t) as usize))
                .copied(),
            varn_types::Value::TaskHandle(th) => {
                self.identity_index.get(&th.identity()).copied()
            }
            varn_types::Value::Generator(g) => self
                .identity_index
                .get(&(Rc::as_ptr(&g.0) as *const () as usize))
                .copied(),
            varn_types::Value::AsyncQueue(q) => self
                .identity_index
                .get(&(Rc::as_ptr(&q.0) as usize))
                .copied(),
            varn_types::Value::VmValue(payload) => {
                if let Some(wrapper) = payload.as_any().downcast_ref::<VmClosurePayload>() {
                    let closure_ptr = std::rc::Rc::as_ptr(&wrapper.0) as *const () as usize;
                    self.identity_index.get(&closure_ptr).copied()
                } else if let Some(vr) = payload.as_any().downcast_ref::<VmValueRef>() {
                    if vr.0.is_heap() {
                        Some(vr.0.as_heap_idx())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn objects_mut(&mut self) -> &mut Vec<Option<HeapObj>> {
        &mut self.objects
    }

    pub fn collect(&mut self, roots: &[u32]) -> Result<usize, crate::gc::GcError> {
        if let Some(mut collector) = self.gc_collector.take() {
            let freed = collector.collect(self, roots)?;
            self.gc_collector = Some(collector);
            self.compact_interners();
            self.rebuild_scan_roots();
            self.gc_collections += 1;
            self.gc_total_freed += freed as u64;
            self.gc_alloc_since_collect = 0;
            Ok(freed)
        } else {
            Ok(0)
        }
    }

    pub fn free_count(&self) -> usize {
        self.free.len()
    }

    pub fn live_count(&self) -> usize {
        self.objects.len().saturating_sub(self.free.len())
    }

    pub fn enable_gc(&mut self) {
        if self.gc_collector.is_none() {
            self.gc_collector = Some(GcCollector::new(self.objects.len() as u32));
        }
    }

    pub fn disable_gc(&mut self) {
        self.gc_collector = None;
    }

    pub fn gc_enabled(&self) -> bool {
        self.gc_collector.is_some()
    }

    pub fn native_module_get(&self, obj: VmValue, key: &str) -> Option<VmValue> {
        let idx = obj.as_heap_idx();
        if let Some(HeapObj::Module(m)) = self.get(idx) {
            let &slot = m.export_map.get(key)?;
            return m.get_slot(slot);
        }
        None
    }
}

#[derive(Clone)]
pub struct Heap {
    inner: Rc<std::cell::UnsafeCell<HeapInner>>,
}

impl Heap {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(std::cell::UnsafeCell::new(HeapInner::new())),
        }
    }

    pub fn deep_clone(&self) -> Self {
        let mut inner_clone = unsafe { (*self.inner.get()).clone() };
        // Same indices, different objects: code baked for the original must
        // not be entered against the copy.
        inner_clone.jit_epoch = crate::clif_link::next_epoch();
        Self {
            inner: Rc::new(std::cell::UnsafeCell::new(inner_clone)),
        }
    }

    /// See [`HeapInner::jit_epoch`].
    #[inline(always)]
    pub fn jit_epoch(&self) -> u64 {
        unsafe { (*self.inner.get()).jit_epoch }
    }

    #[inline(always)]
    pub unsafe fn inner_mut(&self) -> &mut HeapInner {
        &mut *self.inner.get()
    }

    /// Byte offset from the `RcBox` pointer stored in `Heap.inner` to the
    /// nursery's live-object count, for the JIT back-edge safepoint.
    /// `RcBox` is `#[repr(C)] { strong, weak, value }`; the chain is
    /// validated against a live heap in `ExecCtx::new`.
    pub fn nursery_len_byte_offset_from_rcbox() -> usize {
        2 * std::mem::size_of::<usize>()
            + std::mem::offset_of!(HeapInner, nursery)
            + crate::nursery::Nursery::objects_len_byte_offset()
    }

    /// Raw pointer to the `RcBox` behind `inner` (the address JIT code loads
    /// from `ExecCtx.heap`), used only to validate the offset chain.
    pub fn rcbox_ptr_for_validation(&self) -> *const u8 {
        // Rc's data pointer is RcBox + 16; undo that to recover the box base.
        (Rc::as_ptr(&self.inner) as *const u8).wrapping_sub(2 * std::mem::size_of::<usize>())
    }

    /// Probed layout facts for the JIT's inline array-read fast path
    /// (see `varn_jit::JitArrayLayout`). `Vec`'s field order is not
    /// guaranteed by Rust, so the word offsets are discovered against
    /// vectors with known contents; layouts are fixed for the lifetime of
    /// one binary, which is exactly the lifetime of emitted JIT code.
    pub fn jit_array_layout() -> varn_jit::JitArrayLayout {
        // (ptr, len) word offsets inside a Vec<T>: len 3, capacity 7
        // disambiguates all three words (a heap pointer can be neither).
        fn vec_word_offsets<T>(v: &Vec<T>) -> (usize, usize) {
            assert_eq!(v.len(), 3);
            assert_eq!(v.capacity(), 7);
            let words: [usize; 3] = unsafe { std::mem::transmute_copy(v) };
            let ptr = v.as_ptr() as usize;
            let mut ptr_off = usize::MAX;
            let mut len_off = usize::MAX;
            for (i, w) in words.iter().enumerate() {
                if *w == ptr {
                    ptr_off = i * 8;
                } else if *w == 3 {
                    len_off = i * 8;
                }
            }
            assert!(
                ptr_off != usize::MAX && len_off != usize::MAX,
                "Vec layout probe failed"
            );
            (ptr_off, len_off)
        }

        let mut slots_probe: Vec<Option<HeapObj>> = Vec::with_capacity(7);
        for _ in 0..3 {
            slots_probe.push(None);
        }
        let (slots_ptr_off, _slots_len_off) = vec_word_offsets(&slots_probe);

        // Element (data ptr, len) offsets are measured INSIDE an
        // `ArrayRepr::Boxed`, not a bare `Vec`: the JIT reaches the payload at
        // `RcBox + 16`, which now points at the `ArrayRepr` (u8 tag +
        // alignment padding + `Vec`). Measuring against the wrapped repr folds
        // the tag/padding into the offsets, so `payload + 16 + off` lands on
        // the `Vec`'s words exactly as before the wrapping.
        let (disc_off, elems_ptr_off, elems_len_off) = {
            let mut boxed_vec: Vec<VmValue> = Vec::with_capacity(7);
            for _ in 0..3 {
                boxed_vec.push(VmValue::null());
            }
            let vec_ptr = boxed_vec.as_ptr() as usize;
            let repr = varn_types::vm_value::ArrayRepr::Boxed(boxed_vec);
            let repr_size = std::mem::size_of::<varn_types::vm_value::ArrayRepr>();
            let base = &repr as *const _ as *const u8;
            let word = std::mem::size_of::<usize>();

            // `#[repr(C, u8)]`: discriminant is a u8 at offset 0, and Boxed == 0.
            let disc = unsafe { *base };
            assert_eq!(disc, 0, "ArrayRepr::Boxed discriminant must be 0");

            // Scan word-aligned slots from the payload region (skip the tag
            // word to avoid interpreting alignment padding). len 3 / capacity 7
            // disambiguates the three Vec words, exactly as the bare-Vec probe.
            let mut ptr_off = usize::MAX;
            let mut len_off = usize::MAX;
            let mut off = word;
            while off + word <= repr_size {
                let w = unsafe { *(base.add(off) as *const usize) };
                if w == vec_ptr {
                    ptr_off = off;
                } else if w == 3 {
                    len_off = off;
                }
                off += word;
            }
            assert!(
                ptr_off != usize::MAX && len_off != usize::MAX,
                "ArrayRepr::Boxed Vec layout probe failed"
            );

            // Verify: reading back through the probed offsets recovers the real
            // ptr/len. `RcBox + 16` == `&ArrayRepr` (UnsafeCell is transparent),
            // so this exactly mirrors the load the JIT emits.
            let probed_ptr = unsafe { *(base.add(ptr_off) as *const usize) };
            let probed_len = unsafe { *(base.add(len_off) as *const usize) };
            assert_eq!(probed_ptr, vec_ptr, "elems_ptr_off probe read-back mismatch");
            assert_eq!(probed_len, 3, "elems_len_off probe read-back mismatch");

            (0usize, ptr_off, len_off)
        };

        // A.2 / I1: the JIT's array fast path reads `disc_off` to branch on
        // the repr, then (for the typed arms) reads `elems_ptr_off` /
        // `elems_len_off` — offsets measured above against `ArrayRepr::Boxed`
        // only. Task A.4 will start constructing `I64`/`F64` arrays, so
        // check NOW, at startup, that those same offsets also land on the
        // `Vec` payload for the typed variants — turning a layout
        // *assumption* the later phase would otherwise silently rely on into
        // a checked invariant here, before any typed array exists.
        {
            let mut i64_vec: Vec<i64> = Vec::with_capacity(7);
            for _ in 0..3 {
                i64_vec.push(0);
            }
            let i64_ptr = i64_vec.as_ptr() as usize;
            let repr = varn_types::vm_value::ArrayRepr::I64(i64_vec);
            let base = &repr as *const _ as *const u8;
            let disc = unsafe { *base.add(disc_off) };
            assert_eq!(
                disc, 1,
                "ArrayRepr::I64 discriminant must read as 1 at the probed disc_off"
            );
            let probed_ptr = unsafe { *(base.add(elems_ptr_off) as *const usize) };
            let probed_len = unsafe { *(base.add(elems_len_off) as *const usize) };
            assert_eq!(
                probed_ptr, i64_ptr,
                "ArrayRepr::I64 Vec ptr does not land at the Boxed-probed elems_ptr_off"
            );
            assert_eq!(
                probed_len, 3,
                "ArrayRepr::I64 Vec len does not land at the Boxed-probed elems_len_off"
            );
        }
        {
            let mut f64_vec: Vec<f64> = Vec::with_capacity(7);
            for _ in 0..3 {
                f64_vec.push(0.0);
            }
            let f64_ptr = f64_vec.as_ptr() as usize;
            let repr = varn_types::vm_value::ArrayRepr::F64(f64_vec);
            let base = &repr as *const _ as *const u8;
            let disc = unsafe { *base.add(disc_off) };
            assert_eq!(
                disc, 2,
                "ArrayRepr::F64 discriminant must read as 2 at the probed disc_off"
            );
            let probed_ptr = unsafe { *(base.add(elems_ptr_off) as *const usize) };
            let probed_len = unsafe { *(base.add(elems_len_off) as *const usize) };
            assert_eq!(
                probed_ptr, f64_ptr,
                "ArrayRepr::F64 Vec ptr does not land at the Boxed-probed elems_ptr_off"
            );
            assert_eq!(
                probed_len, 3,
                "ArrayRepr::F64 Vec len does not land at the Boxed-probed elems_len_off"
            );
        }

        // Slot probe: discriminant byte and the offset of the payload's Rc
        // pointer inside `Option<HeapObj>` (the Option uses the enum's spare
        // discriminant values as its niche, so the tag byte is shared).
        let arr = varn_types::vm_value::VmArray::new(vec![VmValue::null()]);
        // Rc's data pointer is RcBox + 16; the word stored inside the slot
        // is the RcBox base.
        let rcbox = Rc::as_ptr(&arr.0) as usize - 2 * std::mem::size_of::<usize>();
        let slot: Option<HeapObj> = Some(HeapObj::Array(arr));
        let size = std::mem::size_of::<Option<HeapObj>>();
        let bytes =
            unsafe { std::slice::from_raw_parts(&slot as *const _ as *const u8, size) };
        let array_tag = bytes[0] as usize;
        let payload_off = (0..=size - 8)
            .find(|&off| {
                usize::from_ne_bytes(bytes[off..off + 8].try_into().unwrap()) == rcbox
            })
            .expect("array payload probe failed");

        // The niche assumption: None and non-Array variants must differ in
        // the tag byte the fast path reads.
        let none_slot: Option<HeapObj> = None;
        let none_tag = unsafe { *(&none_slot as *const _ as *const u8) } as usize;
        assert_ne!(array_tag, none_tag, "Option<HeapObj> niche probe failed");

        varn_jit::JitArrayLayout {
            slots_vec_off: 2 * std::mem::size_of::<usize>()
                + std::mem::offset_of!(HeapInner, objects),
            nursery_slots_vec_off: 2 * std::mem::size_of::<usize>()
                + std::mem::offset_of!(HeapInner, nursery)
                + crate::nursery::Nursery::objects_vec_byte_offset(),
            slots_ptr_off,
            slot_size: size,
            array_tag,
            payload_off,
            disc_off,
            elems_ptr_off,
            elems_len_off,
        }
    }

    /// Probed layout facts for the JIT's inline property fast paths (see
    /// [`varn_jit::JitObjectLayout`]).
    ///
    /// Everything is discovered against a real object built with sentinel
    /// values rather than assumed: `#[repr(C)]` pins `ObjData`'s field order,
    /// but the `Rc` header size and the enum's niche placement are still the
    /// compiler's business. The old fast paths hardcoded these as 32/40/48 and
    /// would have kept "working" — into freed memory — after any layout change.
    pub fn jit_object_layout() -> varn_jit::JitObjectLayout {
        // Sentinels chosen so no legitimate pointer, length, or shape id can be
        // confused with them while scanning the block.
        const SENTINEL_FIELD: u64 = 0xFEED_BEEF_CAFE_1234;
        const TAIL: usize = 3;

        let shape = varn_types::Shape::create(None, std::collections::HashMap::new());
        let shape_id = shape.id;
        let oref = ObjRef::with_shape(
            Rc::clone(&shape),
            vec![VmValue(SENTINEL_FIELD); TAIL],
        );

        // What the heap slot stores is the `Rc`'s internal pointer — the RcBox
        // base, which sits two words below the value. Every offset below is
        // relative to that, because that is what the JIT loads from the slot.
        let rcbox = Rc::as_ptr(&oref.0) as *const u8 as usize - 2 * std::mem::size_of::<usize>();
        // Same for the shape: the `Rc<Shape>` stored in the header holds the
        // shape's RcBox base, which is the pointer the JIT will dereference.
        let shape_ptr =
            Rc::as_ptr(&shape) as *const u8 as usize - 2 * std::mem::size_of::<usize>();

        // Read the object's own block: find the tail (the sentinel run), the
        // inline length, and the shape pointer.
        let block = unsafe { std::slice::from_raw_parts(rcbox as *const u8, 80) };
        let word_at = |off: usize| -> u64 {
            u64::from_ne_bytes(block[off..off + 8].try_into().unwrap())
        };

        let values_off = (0..=72)
            .step_by(8)
            .find(|&off| word_at(off) == SENTINEL_FIELD)
            .expect("object tail probe failed: no sentinel field found");
        let shape_off = (0..=72)
            .step_by(8)
            .find(|&off| word_at(off) as usize == shape_ptr)
            .expect("object shape probe failed");
        let len_off = (0..=72)
            .step_by(8)
            .find(|&off| (word_at(off) & 0xFFFF_FFFF) as usize == TAIL && off != values_off)
            .expect("object inline_len probe failed");

        // `Shape.id` is a public field, so its offset is exact rather than
        // scanned for; the RcBox header still has to be added, because what the
        // JIT holds is the box base.
        let shape_id_off =
            2 * std::mem::size_of::<usize>() + std::mem::offset_of!(varn_types::Shape, id);
        assert_eq!(
            unsafe { *((shape_ptr + shape_id_off) as *const u32) },
            shape_id,
            "shape id offset does not resolve to Shape.id"
        );

        // Where the object's RcBox pointer sits inside an `Option<HeapObj>` slot.
        let slot: Option<HeapObj> = Some(HeapObj::Object(oref.clone()));
        let size = std::mem::size_of::<Option<HeapObj>>();
        let bytes =
            unsafe { std::slice::from_raw_parts(&slot as *const _ as *const u8, size) };
        let object_tag = bytes[0] as usize;
        let payload_off = (0..=size - 8)
            .find(|&off| usize::from_ne_bytes(bytes[off..off + 8].try_into().unwrap()) == rcbox)
            .expect("object payload probe failed");

        let none_tag = unsafe { *(&(None::<HeapObj>) as *const _ as *const u8) } as usize;
        assert_ne!(object_tag, none_tag, "Option<HeapObj> niche probe failed");

        varn_jit::JitObjectLayout {
            object_tag,
            payload_off,
            len_off,
            values_off,
            shape_off,
            shape_id_off,
        }
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

impl NativeCtx for Heap {
    fn int_val(&mut self, n: i64) -> VmValue {
        self.deref_mut().make_int(n)
    }

    fn is_int(&self, v: VmValue) -> bool {
        self.deref().is_int(v)
    }

    fn as_int(&self, v: VmValue) -> i64 {
        self.deref().as_int(v)
    }

    fn to_f64(&self, v: VmValue) -> f64 {
        self.deref().to_f64_val(v)
    }

    // Native results are runtime-produced values: allocate without interning
    // (`alloc_str` would hash the full contents and retain a reference on the
    // old-gen path — see `alloc_str_dynamic`'s contract).
    fn alloc_str(&mut self, s: &str) -> VmValue {
        self.deref_mut().alloc_str_dynamic(s)
    }

    fn alloc_str_owned(&mut self, s: String) -> VmValue {
        self.deref_mut().alloc_str_dynamic(&s)
    }

    fn alloc_array(&mut self, items: Vec<VmValue>) -> VmValue {
        self.alloc_array_vm(items)
    }

    fn alloc_object(&mut self) -> VmValue {
        self.deref_mut().alloc_object()
    }

    fn alloc_object_with_shape(
        &mut self,
        shape: &std::rc::Rc<varn_types::Shape>,
        values: Vec<VmValue>,
    ) -> VmValue {
        self.deref_mut().alloc_object_with_shape(shape, values)
    }

    fn alloc_range(&mut self, start: i64, end: i64, inclusive: bool) -> VmValue {
        self.deref_mut().alloc_range(start, end, inclusive)
    }

    fn alloc_fn(&mut self, f: NativeFn, name: &'static str) -> VmValue {
        self.alloc_native_fn(f, name)
    }

    fn alloc_class(&mut self, class: Rc<ClassObj>) -> VmValue {
        self.intern(Value::Class(class))
    }

    fn is_string(&self, v: VmValue) -> bool {
        self.deref().is_string(v)
    }

    fn is_array(&self, v: VmValue) -> bool {
        v.is_heap() && matches!(self.get_by_idx(v.as_heap_idx()), Some(HeapObj::Array(_)))
    }

    fn str_repr(&self, v: VmValue) -> String {
        self.deref().str_repr(v)
    }

    fn str_repr_borrowed<'a>(&'a self, v: VmValue) -> std::borrow::Cow<'a, str> {
        self.deref().str_repr_borrowed(v)
    }

    fn str_owned(&self, v: VmValue) -> Option<String> {
        self.deref().str_owned(v)
    }

    fn str_shared(&self, v: VmValue) -> Option<std::rc::Rc<str>> {
        if v.is_sso() {
            let mut buf = [0u8; 5];
            return Some(std::rc::Rc::from(v.sso_as_str(&mut buf)));
        }
        if v.is_heap() {
            if let Some(HeapObj::Str(s)) = self.get_by_idx(v.as_heap_idx()) {
                return Some(s.to_shared());
            }
        }
        None
    }

    fn array_len(&self, arr: VmValue) -> usize {
        crate::heap_array::array_len(self, arr)
    }

    fn array_get(&self, arr: VmValue, idx: usize) -> Option<VmValue> {
        crate::heap_array::array_get(self, arr, idx)
    }

    fn array_set(&mut self, arr: VmValue, idx: usize, val: VmValue) {
        crate::heap_array::array_set(self, arr, idx, val)
    }

    fn array_push(&mut self, arr: VmValue, val: VmValue) {
        crate::heap_array::array_push(self, arr, val)
    }

    fn array_pop(&mut self, arr: VmValue) -> Option<VmValue> {
        crate::heap_array::array_pop(self, arr)
    }

    fn array_for_each(&self, arr: VmValue, f: &mut dyn FnMut(VmValue, usize)) {
        crate::heap_array::array_for_each(self, arr, f)
    }

    fn is_object(&self, v: VmValue) -> bool {
        if v.is_heap() {
            matches!(self.get_by_idx(v.as_heap_idx()), Some(HeapObj::Object(_)))
        } else {
            false
        }
    }

    fn object_for_each(&self, obj: VmValue, f: &mut dyn FnMut(&str, VmValue)) {
        if obj.is_heap() {
            if let Some(HeapObj::Object(o)) = self.get_by_idx(obj.as_heap_idx()) {
                let g = o.borrow();
                for (k, v) in g.iter() {
                    f(k.as_ref(), v);
                }
            }
        }
    }

    fn get_object_shape(&self, obj: VmValue) -> Option<std::rc::Rc<varn_types::Shape>> {
        if obj.is_heap() {
            if let Some(HeapObj::Object(o)) = self.get_by_idx(obj.as_heap_idx()) {
                return Some(Rc::clone(o.borrow().shape()));
            }
        }
        None
    }

    fn get_field(&self, obj: VmValue, key: &str) -> Option<VmValue> {
        if obj.is_heap() {
            if let Some(HeapObj::Object(o)) = self.get_by_idx(obj.as_heap_idx()) {
                return o.borrow().get_field_nv(key);
            }
            if let Some(HeapObj::Module(m)) = self.get_by_idx(obj.as_heap_idx()) {
                let slot = m.export_map.get(key).copied()?;
                return m.get_slot(slot);
            }
        }
        None
    }

    fn set_field(&mut self, obj: VmValue, key: &str, val: VmValue) {
        if obj.is_heap() {
            let raw_idx = obj.as_heap_idx();
            if let Some(HeapObj::Object(o)) = self.get_by_idx(raw_idx) {
                o.set_field_nv(Rc::from(key), val);
                self.write_barrier(raw_idx, val);
            } else if let Some(HeapObj::Module(m)) = self.get_by_idx_mut(raw_idx) {
                if let Some(s) = m.export_map.get(key).copied() {
                    Rc::make_mut(m).set_slot(s, val);
                } else {
                    let m = Rc::make_mut(m);
                    let slot = m.exports.len();
                    m.exports.push(val);
                    m.export_map.insert(Rc::from(key), slot);
                }
                self.write_barrier(raw_idx, val);
            }
        }
    }

    fn finalize(&mut self, obj: VmValue) -> VmValue {
        obj
    }

    fn call_vm(&mut self, _callee: VmValue, _args: &[VmValue]) -> Result<VmValue, String> {
        Err("call_vm unavailable on bare Heap (use ExecCtx)".into())
    }

    fn spawn_vm(&mut self, _callee: VmValue, _args: &[VmValue]) -> Result<VmValue, String> {
        Err("spawn_vm unavailable on bare Heap (use ExecCtx)".into())
    }

    fn set_timer(
        &mut self,
        _ms: u64,
        _repeat: bool,
        _callee: VmValue,
        _args: &[VmValue],
    ) -> Result<usize, String> {
        Err("set_timer unavailable on bare Heap".into())
    }

    fn clear_timer(&mut self, _id: usize) -> Result<(), String> {
        Ok(())
    }

    fn suspend_timer(&mut self, _ms: u64) -> VmValue {
        VmValue::null()
    }

    fn resources(&mut self) -> &mut ResourceStore {
        panic!("resources() unavailable on bare Heap")
    }

    fn extract(&self, v: VmValue) -> Value {
        self.deref().extract(v)
    }

    fn intern(&mut self, v: Value) -> VmValue {
        self.deref_mut().intern(v)
    }

    fn map_key(&mut self, v: VmValue) -> varn_types::value::MapKey {
        self.deref_mut().canonical_map_key(v)
    }

    // See ExecCtx's impl: keys must canonicalize through the content
    // interner, not the trait default's `intern` (which allocates
    // dynamically for strings).
    fn str_map_key(&mut self, s: &str) -> varn_types::value::MapKey {
        match VmValue::try_from_sso(s) {
            Some(v) => varn_types::value::MapKey(v),
            None => varn_types::value::MapKey(self.deref_mut().alloc_str_interned(s)),
        }
    }

    fn collection_write_barrier(&mut self, parent: VmValue, child: VmValue) {
        if parent.is_heap() {
            self.deref_mut().write_barrier(parent.as_heap_idx(), child);
        }
    }

    fn call_static(&mut self, f: NativeFn) -> VmValue {
        varn_types::call_static_with(self, f)
    }

    fn get_class(&self, name: &str) -> Option<std::rc::Rc<ClassObj>> {
        self.get_intrinsic_class(name)
    }

    fn register_class(&mut self, name: &str, cls: std::rc::Rc<ClassObj>) {
        self.set_intrinsic_class(name, cls);
    }
}

#[cfg(test)]
mod jit_object_layout_tests {
    use super::*;

    /// Walks the exact pointer chain the JIT emits, using only the probed
    /// offsets, and checks it lands on the real field values. A wrong offset
    /// fails here instead of faulting inside generated code.
    #[test]
    fn probed_offsets_reach_the_real_fields() {
        varn_runtime::init_heap();
        let lay = Heap::jit_object_layout();

        let shape = varn_types::Shape::create(None, std::collections::HashMap::new());
        let oref = ObjRef::with_shape(
            Rc::clone(&shape),
            vec![VmValue::from_int(11), VmValue::from_int(22)],
        );
        let slot: Option<HeapObj> = Some(HeapObj::Object(oref.clone()));

        let slot_bytes = unsafe {
            std::slice::from_raw_parts(
                &slot as *const _ as *const u8,
                std::mem::size_of::<Option<HeapObj>>(),
            )
        };
        assert_eq!(slot_bytes[0] as usize, lay.object_tag, "tag byte");

        // slot -> RcBox
        let rc = usize::from_ne_bytes(
            slot_bytes[lay.payload_off..lay.payload_off + 8]
                .try_into()
                .unwrap(),
        );

        let read_u64 = |addr: usize| unsafe { *(addr as *const u64) };
        let read_u32 = |addr: usize| unsafe { *(addr as *const u32) };

        // inline_len, the JIT's bounds check
        assert_eq!(read_u32(rc + lay.len_off) as usize, 2, "inline_len");

        // shape -> shape.id
        let shape_ptr = read_u64(rc + lay.shape_off) as usize;
        assert_eq!(read_u32(shape_ptr + lay.shape_id_off), shape.id, "shape id");

        // values[i] via a constant lea off the data pointer
        assert_eq!(read_u64(rc + lay.values_off), VmValue::from_int(11).0);
        assert_eq!(read_u64(rc + lay.values_off + 8), VmValue::from_int(22).0);
    }
}

/// A.3: exercises the `NativeCtx` `array_*` entry points — the single funnel
/// every interpreter opcode, native builtin (via `VnArray`), and JIT slow
/// path uses to reach array elements — against each `ArrayRepr` variant.
/// Before this task these methods called `VmArray::borrow()/borrow_mut()`,
/// which panics on a typed repr; these tests prove the total-accessor
/// (`get_vm`/`set_vm`/`push_vm`/`pop_vm`/`len`) rewrite is variant-safe and
/// that the Boxed path is unchanged.
#[cfg(test)]
mod array_variant_runtime_tests {
    use super::*;

    fn array_discriminant(heap: &Heap, nv: VmValue) -> u8 {
        match heap.get(nv.as_heap_idx()) {
            Some(HeapObj::Array(a)) => a.discriminant(),
            other => panic!("expected Array, got {other:?}"),
        }
    }

    #[test]
    fn array_helpers_i64_variant_get_set_push_pop() {
        let mut heap = Heap::new();
        let raw = unsafe { heap.inner_mut() }
            .alloc_raw(HeapObj::Array(VmArray::new_i64(vec![1, 2, 3])));
        let arr_nv = VmValue::from_heap_idx(pack_old_idx(raw));

        assert_eq!(heap.array_len(arr_nv), 3);
        assert_eq!(heap.array_get(arr_nv, 1).unwrap().as_int(), 2);
        assert!(heap.array_get(arr_nv, 99).is_none());

        // Matching-typed set/push stay I64 — no migration.
        heap.array_set(arr_nv, 0, VmValue::from_int(100));
        assert_eq!(heap.array_get(arr_nv, 0).unwrap().as_int(), 100);
        assert_eq!(array_discriminant(&heap, arr_nv), 1);

        heap.array_push(arr_nv, VmValue::from_int(4));
        assert_eq!(heap.array_len(arr_nv), 4);
        assert_eq!(array_discriminant(&heap, arr_nv), 1);

        let popped = heap.array_pop(arr_nv).unwrap();
        assert_eq!(popped.as_int(), 4);
        assert_eq!(heap.array_len(arr_nv), 3);

        let mut seen = Vec::new();
        heap.array_for_each(arr_nv, &mut |v, i| seen.push((i, v.as_int())));
        assert_eq!(seen, vec![(0, 100), (1, 2), (2, 3)]);
    }

    #[test]
    fn array_helpers_f64_variant_get_set_push_pop() {
        let mut heap = Heap::new();
        let raw = unsafe { heap.inner_mut() }
            .alloc_raw(HeapObj::Array(VmArray::new_f64(vec![1.5, 2.5])));
        let arr_nv = VmValue::from_heap_idx(pack_old_idx(raw));

        assert_eq!(heap.array_len(arr_nv), 2);
        assert_eq!(heap.array_get(arr_nv, 1).unwrap().as_f64(), 2.5);

        // Matching-typed set/push stay F64 — no migration.
        heap.array_set(arr_nv, 0, VmValue::from_f64(9.5));
        assert_eq!(heap.array_get(arr_nv, 0).unwrap().as_f64(), 9.5);
        assert_eq!(array_discriminant(&heap, arr_nv), 2);

        heap.array_push(arr_nv, VmValue::from_f64(3.5));
        assert_eq!(heap.array_len(arr_nv), 3);
        assert_eq!(array_discriminant(&heap, arr_nv), 2);

        let popped = heap.array_pop(arr_nv).unwrap();
        assert_eq!(popped.as_f64(), 3.5);
    }

    /// `alloc_array_vm` — the single construction point for arrays built from
    /// boxed values — picks the repr from those values. An all-int literal
    /// gets a raw `Vec<i64>`, and every accessor still round-trips the exact
    /// `VmValue` that was stored.
    #[test]
    fn alloc_array_vm_picks_i64_repr_for_all_int_items() {
        let mut heap = Heap::new();
        let arr_nv = heap.alloc_array_vm(vec![VmValue::from_int(10), VmValue::from_int(20)]);
        assert_eq!(array_discriminant(&heap, arr_nv), 1);

        assert_eq!(heap.array_len(arr_nv), 2);
        assert_eq!(heap.array_get(arr_nv, 1).unwrap().as_int(), 20);
        heap.array_set(arr_nv, 0, VmValue::from_int(99));
        assert_eq!(heap.array_get(arr_nv, 0).unwrap().as_int(), 99);
        heap.array_push(arr_nv, VmValue::from_int(7));
        assert_eq!(heap.array_len(arr_nv), 3);
        assert_eq!(heap.array_pop(arr_nv).unwrap().as_int(), 7);
        assert_eq!(array_discriminant(&heap, arr_nv), 1);
    }

    /// A heterogeneous literal has no raw repr to fall into, so it stays
    /// `Boxed` and behaves exactly as it always did.
    #[test]
    fn array_helpers_boxed_variant_unchanged() {
        let mut heap = Heap::new();
        let s = heap.alloc_str_dynamic("a string longer than five bytes");
        let arr_nv = heap.alloc_array_vm(vec![VmValue::from_int(10), s]);
        assert_eq!(array_discriminant(&heap, arr_nv), 0);

        assert_eq!(heap.array_len(arr_nv), 2);
        assert_eq!(heap.array_get(arr_nv, 1).unwrap(), s);
        heap.array_set(arr_nv, 0, VmValue::from_int(99));
        assert_eq!(heap.array_get(arr_nv, 0).unwrap().as_int(), 99);
        heap.array_push(arr_nv, VmValue::from_int(7));
        assert_eq!(heap.array_len(arr_nv), 3);
        assert_eq!(heap.array_pop(arr_nv).unwrap().as_int(), 7);
        assert_eq!(array_discriminant(&heap, arr_nv), 0);
    }

    /// The `let a = []` + `a.push(...)` shape: an empty array has no repr to
    /// preserve, so its first push decides one. This is what makes arrays
    /// built by pushing — the overwhelmingly common case — unboxed.
    #[test]
    fn empty_array_specializes_on_first_push() {
        let mut heap = Heap::new();

        let ints = heap.alloc_array_vm(vec![]);
        assert_eq!(array_discriminant(&heap, ints), 0);
        heap.array_push(ints, VmValue::from_int(1));
        assert_eq!(array_discriminant(&heap, ints), 1);
        heap.array_push(ints, VmValue::from_int(2));
        assert_eq!(heap.array_len(ints), 2);
        assert_eq!(heap.array_get(ints, 1).unwrap().as_int(), 2);

        let floats = heap.alloc_array_vm(vec![]);
        heap.array_push(floats, VmValue::from_f64(1.5));
        assert_eq!(array_discriminant(&heap, floats), 2);
        assert_eq!(heap.array_get(floats, 0).unwrap().as_f64(), 1.5);

        // A non-numeric first push has no raw repr: stays Boxed.
        let s = heap.alloc_str_dynamic("a string longer than five bytes");
        let strs = heap.alloc_array_vm(vec![]);
        heap.array_push(strs, s);
        assert_eq!(array_discriminant(&heap, strs), 0);
        assert_eq!(heap.array_get(strs, 0).unwrap(), s);
    }

    /// A specialized array that later takes a mismatched push migrates back to
    /// `Boxed`, keeping every element it had already accepted.
    #[test]
    fn specialized_array_migrates_back_on_mismatched_push() {
        let mut heap = Heap::new();
        let arr = heap.alloc_array_vm(vec![]);
        heap.array_push(arr, VmValue::from_int(1));
        heap.array_push(arr, VmValue::from_int(2));
        assert_eq!(array_discriminant(&heap, arr), 1);

        let s = heap.alloc_str_dynamic("a string longer than five bytes");
        heap.array_push(arr, s);
        assert_eq!(array_discriminant(&heap, arr), 0);
        assert_eq!(heap.array_len(arr), 3);
        assert_eq!(heap.array_get(arr, 0).unwrap().as_int(), 1);
        assert_eq!(heap.array_get(arr, 1).unwrap().as_int(), 2);
        assert_eq!(heap.array_get(arr, 2).unwrap(), s);
    }

    /// A.2/A.3 binding contract: a type-mismatched `array_set` into a typed
    /// repr migrates the array to `Boxed` in place (identity preserved) and
    /// must still fire the write barrier exactly as the pre-existing Boxed
    /// `array_set` path does — the accessor swap must not silently drop the
    /// barrier call for the migration case.
    #[test]
    fn array_set_migration_preserves_write_barrier_semantics() {
        let mut heap = Heap::new();

        // Old-gen typed array (bypasses the nursery, like a promoted array
        // would be) so `write_barrier`'s `is_old_idx(parent)` check is live.
        let arr_raw = unsafe { heap.inner_mut() }
            .alloc_raw(HeapObj::Array(VmArray::new_i64(vec![1, 2, 3])));
        let arr_packed = pack_old_idx(arr_raw);
        assert!(is_old_idx(arr_packed));
        let arr_nv = VmValue::from_heap_idx(arr_packed);

        // A nursery-allocated string: writing it into the array is both a
        // type mismatch (forces migration to Boxed) and a nursery child of
        // an old-gen parent (must be remembered by the barrier).
        let child_nv = heap.alloc_str_dynamic("a string longer than five bytes");
        assert!(child_nv.is_heap());
        assert!(is_nursery_idx(child_nv.as_heap_idx()));

        heap.array_set(arr_nv, 1, child_nv);

        // Migrated to Boxed; identity and the other elements are preserved.
        assert_eq!(array_discriminant(&heap, arr_nv), 0);
        assert_eq!(heap.array_get(arr_nv, 0).unwrap().as_int(), 1);
        assert_eq!(heap.array_get(arr_nv, 1).unwrap(), child_nv);
        assert_eq!(heap.array_get(arr_nv, 2).unwrap().as_int(), 3);

        // The write barrier ran: the old-gen array's slot is in the
        // nursery's remembered set, so a minor GC (which only walks
        // remembered old-gen slots, not every old-gen object) still finds
        // the array->string edge and keeps the string alive.
        let remembered = unsafe { heap.inner_mut() }.nursery.remembered.clone();
        assert!(
            remembered.contains(&arr_packed),
            "write_barrier did not remember the migrated array's old-gen slot; \
             a nursery-promoted GC pass would miss the array->string edge"
        );
    }

    /// OOB `array_set` must not fire the write barrier — matches the
    /// pre-existing Boxed behavior (`idx < len` gate) exactly, now driven by
    /// `set_vm`'s bool return instead of a manual length check.
    #[test]
    fn array_set_out_of_bounds_does_not_migrate_or_remember() {
        let mut heap = Heap::new();
        let arr_raw = unsafe { heap.inner_mut() }
            .alloc_raw(HeapObj::Array(VmArray::new_i64(vec![1, 2, 3])));
        let arr_packed = pack_old_idx(arr_raw);
        let arr_nv = VmValue::from_heap_idx(arr_packed);

        let child_nv = heap.alloc_str_dynamic("a string longer than five bytes");
        heap.array_set(arr_nv, 99, child_nv); // OOB: no-op

        assert_eq!(array_discriminant(&heap, arr_nv), 1); // still I64
        assert_eq!(heap.array_len(arr_nv), 3);
        let remembered = unsafe { heap.inner_mut() }.nursery.remembered.clone();
        assert!(!remembered.contains(&arr_packed));
    }
}
