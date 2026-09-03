use super::{RuntimeString, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_CLASS_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Debug)]
pub struct ClassObj {
    pub id: u32,
    pub name: String,
    pub is_native: bool,
    pub superclass: RefCell<Option<Rc<ClassObj>>>,
    pub vtable: RefCell<Vec<Value>>,
    pub vtable_owners: RefCell<Vec<Option<Rc<ClassObj>>>>,
    pub method_map: RefCell<HashMap<RuntimeString, usize>>,
    pub statics: RefCell<HashMap<RuntimeString, Value>>,
    pub static_fields: RefCell<Vec<RuntimeString>>,
    pub vtable_version: AtomicU32,
    pub root_shape: RefCell<Rc<super::shape::Shape>>,
    pub getter_map: RefCell<HashMap<RuntimeString, usize>>,
    pub getter_vtable: RefCell<Vec<Value>>,
    pub getter_vtable_owners: RefCell<Vec<Option<Rc<ClassObj>>>>,
    pub setter_map: RefCell<HashMap<RuntimeString, usize>>,
    pub setter_vtable: RefCell<Vec<Value>>,
    pub setter_vtable_owners: RefCell<Vec<Option<Rc<ClassObj>>>>,
    pub static_getter_map: RefCell<HashMap<RuntimeString, Value>>,
    pub static_setter_map: RefCell<HashMap<RuntimeString, Value>>,
    /// Cached `constructor` lookup keyed by vtable_version, so hot `new`
    /// paths skip the method_map hash lookup on every instantiation.
    pub ctor_cache: RefCell<Option<(u32, Option<Value>)>>,
    /// VM-owned constructor cache keyed by vtable_version: the VM stores its
    /// resolved closure here (`Rc<VmClosure>` as `Rc<dyn Any>`) so hot `new`
    /// paths skip the `Value` box clone + downcast. `Some((v, None))` means
    /// "class has no constructor". varn-types only provides the slot.
    pub ctor_rt_cache: RefCell<Option<CtorRtCacheEntry>>,
    /// Cached instance shape and inline field count. In Varn, classes have
    /// fixed, immutable field layouts once declared.
    pub instance_shape_cache: RefCell<Option<(Rc<super::shape::Shape>, usize)>>,
}

pub type CtorRtCacheEntry = (u32, Option<Rc<dyn std::any::Any>>);

impl ClassObj {
    pub fn new(name: impl Into<String>) -> Self {
        ClassObj {
            id: NEXT_CLASS_ID.fetch_add(1, Ordering::Relaxed),
            name: name.into(),
            is_native: false,
            superclass: RefCell::new(None),
            vtable_version: AtomicU32::new(1),
            vtable: RefCell::new(Vec::new()),
            vtable_owners: RefCell::new(Vec::new()),
            method_map: RefCell::new(HashMap::new()),
            statics: RefCell::new(HashMap::new()),
            static_fields: RefCell::new(Vec::new()),
            root_shape: RefCell::new(super::shape::root_shape()),
            getter_map: RefCell::new(HashMap::new()),
            getter_vtable: RefCell::new(Vec::new()),
            getter_vtable_owners: RefCell::new(Vec::new()),
            setter_map: RefCell::new(HashMap::new()),
            setter_vtable: RefCell::new(Vec::new()),
            setter_vtable_owners: RefCell::new(Vec::new()),
            static_getter_map: RefCell::new(HashMap::new()),
            static_setter_map: RefCell::new(HashMap::new()),
            ctor_cache: RefCell::new(None),
            ctor_rt_cache: RefCell::new(None),
            instance_shape_cache: RefCell::new(None),
        }
    }
    pub fn new_native(name: impl Into<String>) -> Self {
        let mut cls = Self::new(name);
        cls.is_native = true;
        cls
    }

    pub fn new_rc(name: impl Into<String>) -> Rc<Self> {
        let cls = Rc::new(Self::new(name));
        cls.init_root_shape();
        cls
    }

    pub fn new_native_rc(name: impl Into<String>) -> Rc<Self> {
        let cls = Rc::new(Self::new_native(name));
        cls.init_root_shape();
        cls
    }

    pub fn init_root_shape(self: &Rc<Self>) {
        let mut root = self.root_shape.borrow_mut();
        if root.class.is_none() {
            *root = super::shape::Shape::create(Some(self.clone()), HashMap::new());
        }
    }

    pub fn instance_shape(&self) -> (Rc<super::shape::Shape>, usize) {
        if let Some(ref cached) = *self.instance_shape_cache.borrow() {
            return (Rc::clone(&cached.0), cached.1);
        }
        let shape = self.root_shape.borrow().clone();
        let n = shape.property_names.len();
        *self.instance_shape_cache.borrow_mut() = Some((Rc::clone(&shape), n));
        (shape, n)
    }

    pub fn declare_field(&self, name: RuntimeString) -> usize {
        *self.instance_shape_cache.borrow_mut() = None;
        let mut root = self.root_shape.borrow_mut();
        if let Some(&slot) = root.property_names.get(&name) {
            return slot;
        }
        let new_shape = root.transition(name);
        let slot = new_shape.property_names.len() - 1;
        *root = new_shape;
        slot
    }

    pub fn add_method(&self, name: impl Into<Rc<str>>, value: Value) {
        self.add_method_with_owner(name, value, None);
    }

    pub fn add_method_with_owner(
        &self,
        name: impl Into<Rc<str>>,
        value: Value,
        owner: Option<Rc<ClassObj>>,
    ) {
        let name: Rc<str> = name.into();
        let mut method_map = self.method_map.borrow_mut();
        let mut vtable = self.vtable.borrow_mut();
        let mut vtable_owners = self.vtable_owners.borrow_mut();
        if let Some(&idx) = method_map.get(&name) {
            vtable[idx] = value;
            vtable_owners[idx] = owner;
        } else {
            let idx = vtable.len();
            vtable.push(value);
            vtable_owners.push(owner);
            method_map.insert(name, idx);
        }
        drop((method_map, vtable, vtable_owners));
        self.vtable_version.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_getter(&self, name: impl Into<Rc<str>>, value: Value) {
        self.add_getter_with_owner(name, value, None);
    }

    pub fn add_getter_with_owner(
        &self,
        name: impl Into<Rc<str>>,
        value: Value,
        owner: Option<Rc<ClassObj>>,
    ) {
        let name: Rc<str> = name.into();
        let mut getter_map = self.getter_map.borrow_mut();
        let mut getter_vtable = self.getter_vtable.borrow_mut();
        let mut getter_vtable_owners = self.getter_vtable_owners.borrow_mut();
        if let Some(&idx) = getter_map.get(&name) {
            getter_vtable[idx] = value;
            getter_vtable_owners[idx] = owner;
        } else {
            let idx = getter_vtable.len();
            getter_vtable.push(value);
            getter_vtable_owners.push(owner);
            getter_map.insert(name, idx);
        }
        drop((getter_map, getter_vtable, getter_vtable_owners));
        self.vtable_version.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_setter_with_owner(
        &self,
        name: impl Into<Rc<str>>,
        value: Value,
        owner: Option<Rc<ClassObj>>,
    ) {
        let name: Rc<str> = name.into();
        let mut setter_map = self.setter_map.borrow_mut();
        let mut setter_vtable = self.setter_vtable.borrow_mut();
        let mut setter_vtable_owners = self.setter_vtable_owners.borrow_mut();
        if let Some(&idx) = setter_map.get(&name) {
            setter_vtable[idx] = value;
            setter_vtable_owners[idx] = owner;
        } else {
            let idx = setter_vtable.len();
            setter_vtable.push(value);
            setter_vtable_owners.push(owner);
            setter_map.insert(name, idx);
        }
        drop((setter_map, setter_vtable, setter_vtable_owners));
        self.vtable_version.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_static_getter(&self, name: impl Into<Rc<str>>, value: Value) {
        let name_rc = name.into();
        let mut fields = self.static_fields.borrow_mut();
        if !fields.contains(&name_rc) {
            fields.push(name_rc.clone());
        }
        self.static_getter_map.borrow_mut().insert(name_rc, value);
    }

    pub fn add_static_setter(&self, name: impl Into<Rc<str>>, value: Value) {
        let name_rc = name.into();
        let mut fields = self.static_fields.borrow_mut();
        if !fields.contains(&name_rc) {
            fields.push(name_rc.clone());
        }
        self.static_setter_map.borrow_mut().insert(name_rc, value);
    }

    pub fn add_static(&self, name: impl Into<Rc<str>>, value: Value) {
        let name_rc = name.into();
        let mut fields = self.static_fields.borrow_mut();
        if !fields.contains(&name_rc) {
            fields.push(name_rc.clone());
        }
        self.statics.borrow_mut().insert(name_rc, value);
    }

    pub fn get_static(&self, name: &str) -> Option<Value> {
        self.statics.borrow().get(name).cloned()
    }

    pub fn find_getter(&self, name: &str) -> Option<Value> {
        if let Some(&idx) = self.getter_map.borrow().get(name) {
            return Some(self.getter_vtable.borrow()[idx].clone());
        }
        self.superclass.borrow().as_ref()?.find_getter(name)
    }

    pub fn find_setter(&self, name: &str) -> Option<Value> {
        if let Some(&idx) = self.setter_map.borrow().get(name) {
            return Some(self.setter_vtable.borrow()[idx].clone());
        }
        self.superclass.borrow().as_ref()?.find_setter(name)
    }

    pub fn find_static_getter(&self, name: &str) -> Option<Value> {
        if let Some(v) = self.static_getter_map.borrow().get(name) {
            return Some(v.clone());
        }
        self.superclass.borrow().as_ref()?.find_static_getter(name)
    }

    pub fn find_static_setter(&self, name: &str) -> Option<Value> {
        if let Some(v) = self.static_setter_map.borrow().get(name) {
            return Some(v.clone());
        }
        self.superclass.borrow().as_ref()?.find_static_setter(name)
    }

    pub fn find_method(&self, name: &str) -> Option<Value> {
        if let Some(&idx) = self.method_map.borrow().get(name) {
            return Some(self.vtable.borrow()[idx].clone());
        }
        if let Some(super_cls) = &*self.superclass.borrow() {
            return super_cls.find_method(name);
        }
        None
    }

    /// Cached `find_method("constructor")`, invalidated by vtable_version.
    pub fn constructor(&self) -> Option<Value> {
        let ver = self.vtable_version.load(Ordering::Relaxed);
        if let Some((cached_ver, ctor)) = self.ctor_cache.borrow().as_ref() {
            if *cached_ver == ver {
                return ctor.clone();
            }
        }
        let ctor = self.find_method("constructor");
        *self.ctor_cache.borrow_mut() = Some((ver, ctor.clone()));
        ctor
    }
}

pub fn find_method_with_owner(class: &Rc<ClassObj>, name: &str) -> Option<(Value, Rc<ClassObj>)> {
    if let Some(&idx) = class.method_map.borrow().get(name) {
        return Some((class.vtable.borrow()[idx].clone(), class.clone()));
    }
    if let Some(super_cls) = &*class.superclass.borrow() {
        return find_method_with_owner(super_cls, name);
    }
    None
}
