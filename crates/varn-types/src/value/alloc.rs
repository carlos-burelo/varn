use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

pub type RuntimeString = Rc<str>;

/// Canonicalized `Map`/`Set` key. The wrapped `VmValue`'s bit pattern is
/// content-canonical — produced by `Heap::canonical_map_key`: SSO/int/bool/
/// null/float(-0 normalized) are canonical by representation; heap strings,
/// chars, decimals and bigints canonicalize through the content interners
/// (old-gen, so minor GC never moves them); everything else keys by
/// identity (its packed heap index). Hash and equality are therefore plain
/// u64 operations — no heap access, no fat-enum walk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapKey(pub crate::vm_value::VmValue);

impl std::hash::Hash for MapKey {
    #[inline(always)]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Delegates to `VmValue`'s bit hash so that widening the value
        // representation is a change to `vm_value.rs` and not to this map.
        self.0.hash(state);
    }
}

/// Backing storage for the language's `Map`/`Set`. FxHash: these are
/// single-user, non-adversarial containers, and SipHash dominated lookup
/// cost. Keys are canonicalized `VmValue`s; values are raw `VmValue`s.
pub type ValueMap = rustc_hash::FxHashMap<MapKey, crate::vm_value::VmValue>;
pub type ValueSet = rustc_hash::FxHashSet<MapKey>;

/// Handle to a property object. The fields live inside this same allocation
/// (see `ObjData`), so there is no inner `RefCell` and no second buffer:
/// mutation goes through `ObjData`'s cells on `&self`.
#[derive(Clone)]
pub struct ObjRef(pub Rc<super::ObjData>);

impl ObjRef {
    /// Empty object on the root shape.
    pub fn empty() -> Self {
        Self(super::ObjData::new())
    }

    pub fn instance(class: &super::ClassObj) -> Self {
        Self(super::ObjData::new_instance(class))
    }

    pub fn instance_rc(class: Rc<super::ClassObj>) -> Self {
        Self(super::ObjData::new_instance(&class))
    }

    pub fn with_shape(shape: Rc<super::Shape>, values: Vec<crate::vm_value::VmValue>) -> Self {
        Self(super::ObjData::with_shape(shape, values))
    }

    /// As [`Self::with_shape`], from a borrowed buffer — see
    /// [`super::ObjData::with_shape_slice`].
    pub fn with_shape_slice(shape: Rc<super::Shape>, values: &[crate::vm_value::VmValue]) -> Self {
        Self(super::ObjData::with_shape_slice(shape, values))
    }

    pub fn from_pairs<I>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (RuntimeString, crate::vm_value::VmValue)>,
    {
        Self(super::ObjData::from_pairs(pairs))
    }

    pub fn read(&self) -> &super::ObjData {
        &self.0
    }

    pub fn borrow(&self) -> &super::ObjData {
        &self.0
    }
}

impl std::ops::Deref for ObjRef {
    type Target = super::ObjData;
    #[inline(always)]
    fn deref(&self) -> &super::ObjData {
        &self.0
    }
}

impl PartialEq for ObjRef {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ObjRef {}

impl std::hash::Hash for ObjRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (Rc::as_ptr(&self.0) as *const u8).hash(state);
    }
}

impl std::fmt::Debug for ObjRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ObjRef({:p})", Rc::as_ptr(&self.0) as *const u8)
    }
}

use std::cell::UnsafeCell;

#[derive(Clone)]
pub struct ArrayRef(pub Rc<UnsafeCell<Vec<super::Value>>>);

impl ArrayRef {
    pub fn new(data: Vec<super::Value>) -> Self {
        Self(Rc::new(UnsafeCell::new(data)))
    }

    pub fn read(&self) -> &Vec<super::Value> {
        unsafe { &*self.0.get() }
    }

    /// # Why `mut_from_ref` is allowed here
    ///
    /// Handing out `&mut` from `&self` is exactly what `UnsafeCell` is for, and
    /// it is the representation this VM is built on: object identity is the
    /// address of the `Rc` (see `ObjRef`), so a value can never be reallocated
    /// to be mutated. The safety argument is the VM's single-threaded
    /// execution, not the borrow checker.
    ///
    /// Allowed at the site rather than for the workspace, so a NEW
    /// `&mut`-from-`&self` somewhere that has not made this argument still
    /// fails the build.
    #[allow(clippy::mut_from_ref)]
    pub fn write(&self) -> &mut Vec<super::Value> {
        unsafe { &mut *self.0.get() }
    }

    pub fn borrow(&self) -> &Vec<super::Value> {
        unsafe { &*self.0.get() }
    }

    /// Same interior-mutability contract as [`Self::write`].
    #[allow(clippy::mut_from_ref)]
    pub fn borrow_mut(&self) -> &mut Vec<super::Value> {
        unsafe { &mut *self.0.get() }
    }

    pub fn len(&self) -> usize {
        self.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.borrow().is_empty()
    }
}

impl PartialEq for ArrayRef {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ArrayRef {}

impl std::hash::Hash for ArrayRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}

impl std::fmt::Debug for ArrayRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ArrayRef({:p})", self.0)
    }
}

#[derive(Clone)]
pub struct MapRef(pub Rc<RefCell<ValueMap>>);

impl MapRef {
    pub fn new(data: ValueMap) -> Self {
        Self(Rc::new(RefCell::new(data)))
    }

    pub fn read(&self) -> std::cell::Ref<'_, ValueMap> {
        self.0.borrow()
    }

    pub fn write(&self) -> std::cell::RefMut<'_, ValueMap> {
        self.0.borrow_mut()
    }

    pub fn borrow(&self) -> std::cell::Ref<'_, ValueMap> {
        self.0.borrow()
    }

    pub fn borrow_mut(&self) -> std::cell::RefMut<'_, ValueMap> {
        self.0.borrow_mut()
    }
}

impl PartialEq for MapRef {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for MapRef {}

impl std::hash::Hash for MapRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}

impl std::fmt::Debug for MapRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MapRef({:p})", self.0)
    }
}

#[derive(Clone)]
pub struct SetRef(pub Rc<RefCell<ValueSet>>);

impl SetRef {
    pub fn new(data: ValueSet) -> Self {
        Self(Rc::new(RefCell::new(data)))
    }

    pub fn read(&self) -> std::cell::Ref<'_, ValueSet> {
        self.0.borrow()
    }

    pub fn write(&self) -> std::cell::RefMut<'_, ValueSet> {
        self.0.borrow_mut()
    }

    pub fn borrow(&self) -> std::cell::Ref<'_, ValueSet> {
        self.0.borrow()
    }

    pub fn borrow_mut(&self) -> std::cell::RefMut<'_, ValueSet> {
        self.0.borrow_mut()
    }
}

impl PartialEq for SetRef {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SetRef {}

impl std::hash::Hash for SetRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}

impl std::fmt::Debug for SetRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SetRef({:p})", self.0)
    }
}

pub struct AllocVtable {
    pub alloc_object: fn() -> ObjRef,
    pub alloc_array: fn() -> ArrayRef,
    pub alloc_map: fn() -> MapRef,
    pub alloc_set: fn() -> SetRef,
}

static GLOBAL_VTABLE: OnceLock<&'static AllocVtable> = OnceLock::new();

pub fn register_global_vtable(v: &'static AllocVtable) {
    let _ = GLOBAL_VTABLE.set(v);
}

pub fn init_thread_heap() {
    if let Some(v) = GLOBAL_VTABLE.get() {
        install_allocator(v);
    }
}

pub fn get_global_vtable() -> Option<&'static AllocVtable> {
    GLOBAL_VTABLE.get().copied()
}

fn uninitialized_panic(what: &str) -> ! {
    panic!(
        "Varn heap not initialized — heap::init_heap() must be called before allocating a {what}"
    )
}
fn uninitialized_obj() -> ObjRef {
    uninitialized_panic(varn_core::TypeTag::Object.name())
}
fn uninitialized_arr() -> ArrayRef {
    uninitialized_panic(varn_core::IntrinsicType::Array.as_str())
}
fn uninitialized_map() -> MapRef {
    uninitialized_panic(varn_core::IntrinsicType::Map.as_str())
}
fn uninitialized_set() -> SetRef {
    uninitialized_panic(varn_core::IntrinsicType::Set.as_str())
}

use std::cell::Cell;
thread_local! {
    static TL_VTABLE: Cell<*const AllocVtable> = const { Cell::new(std::ptr::null()) };
}

pub fn install_allocator(v: &'static AllocVtable) {
    TL_VTABLE.with(|c| c.set(v as *const AllocVtable));
}

#[inline(always)]
fn get_vtable() -> &'static AllocVtable {
    let ptr = TL_VTABLE.with(|c| c.get());
    if ptr.is_null() {
        static UNINITIALIZED_VTABLE: AllocVtable = AllocVtable {
            alloc_object: uninitialized_obj,
            alloc_array: uninitialized_arr,
            alloc_map: uninitialized_map,
            alloc_set: uninitialized_set,
        };
        return &UNINITIALIZED_VTABLE;
    }
    unsafe { &*ptr }
}

#[inline(always)]
pub fn alloc_object() -> ObjRef {
    (get_vtable().alloc_object)()
}

#[inline(always)]
pub fn alloc_array() -> ArrayRef {
    (get_vtable().alloc_array)()
}

#[inline(always)]
pub fn alloc_map() -> MapRef {
    (get_vtable().alloc_map)()
}

#[inline(always)]
pub fn alloc_set() -> SetRef {
    (get_vtable().alloc_set)()
}
