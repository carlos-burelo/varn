use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::OnceLock;

pub type RuntimeString = Rc<str>;

#[derive(Clone)]
pub struct ObjRef(pub Rc<RefCell<super::ObjData>>);

impl ObjRef {
    pub fn new(data: super::ObjData) -> Self {
        Self(Rc::new(RefCell::new(data)))
    }

    pub fn read(&self) -> std::cell::Ref<'_, super::ObjData> {
        self.0.borrow()
    }

    pub fn write(&self) -> std::cell::RefMut<'_, super::ObjData> {
        self.0.borrow_mut()
    }

    pub fn borrow(&self) -> std::cell::Ref<'_, super::ObjData> {
        self.0.borrow()
    }

    pub fn borrow_mut(&self) -> std::cell::RefMut<'_, super::ObjData> {
        self.0.borrow_mut()
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
        Rc::as_ptr(&self.0).hash(state);
    }
}

impl std::fmt::Debug for ObjRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ObjRef({:p})", self.0)
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

    pub fn write(&self) -> &mut Vec<super::Value> {
        unsafe { &mut *self.0.get() }
    }

    pub fn borrow(&self) -> &Vec<super::Value> {
        unsafe { &*self.0.get() }
    }

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
pub struct MapRef(pub Rc<RefCell<HashMap<super::Value, super::Value>>>);

impl MapRef {
    pub fn new(data: HashMap<super::Value, super::Value>) -> Self {
        Self(Rc::new(RefCell::new(data)))
    }

    pub fn read(&self) -> std::cell::Ref<'_, HashMap<super::Value, super::Value>> {
        self.0.borrow()
    }

    pub fn write(&self) -> std::cell::RefMut<'_, HashMap<super::Value, super::Value>> {
        self.0.borrow_mut()
    }

    pub fn borrow(&self) -> std::cell::Ref<'_, HashMap<super::Value, super::Value>> {
        self.0.borrow()
    }

    pub fn borrow_mut(&self) -> std::cell::RefMut<'_, HashMap<super::Value, super::Value>> {
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
pub struct SetRef(pub Rc<RefCell<HashSet<super::Value>>>);

impl SetRef {
    pub fn new(data: HashSet<super::Value>) -> Self {
        Self(Rc::new(RefCell::new(data)))
    }

    pub fn read(&self) -> std::cell::Ref<'_, HashSet<super::Value>> {
        self.0.borrow()
    }

    pub fn write(&self) -> std::cell::RefMut<'_, HashSet<super::Value>> {
        self.0.borrow_mut()
    }

    pub fn borrow(&self) -> std::cell::Ref<'_, HashSet<super::Value>> {
        self.0.borrow()
    }

    pub fn borrow_mut(&self) -> std::cell::RefMut<'_, HashSet<super::Value>> {
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
