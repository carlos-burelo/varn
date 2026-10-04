use super::RuntimeString;
use crate::vm_value::VmValue;
use rustc_hash::FxHashMap as HashMap;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

static NEXT_CLASS_ID: AtomicU32 = AtomicU32::new(1);

thread_local! {
    static CLASS_REGISTRY: RefCell<HashMap<u32, std::rc::Weak<ClassObj>>> =
        RefCell::new(HashMap::default());
}

#[derive(Debug)]
pub struct ClassObj {
    pub id: u32,
    pub name: String,
    pub is_native: bool,
    pub superclass: RefCell<Option<Rc<ClassObj>>>,
    pub vtable: RefCell<Vec<VmValue>>,
    pub vtable_owners: RefCell<Vec<Option<Rc<ClassObj>>>>,
    pub method_map: RefCell<HashMap<RuntimeString, usize>>,
    pub statics: RefCell<HashMap<RuntimeString, VmValue>>,
    pub static_fields: RefCell<Vec<RuntimeString>>,
    pub vtable_version: AtomicU32,
    pub root_shape: RefCell<Rc<super::shape::Shape>>,
    pub getter_map: RefCell<HashMap<RuntimeString, usize>>,
    pub getter_vtable: RefCell<Vec<VmValue>>,
    pub getter_vtable_owners: RefCell<Vec<Option<Rc<ClassObj>>>>,
    pub setter_map: RefCell<HashMap<RuntimeString, usize>>,
    pub setter_vtable: RefCell<Vec<VmValue>>,
    pub setter_vtable_owners: RefCell<Vec<Option<Rc<ClassObj>>>>,
    pub static_getter_map: RefCell<HashMap<RuntimeString, VmValue>>,
    pub static_setter_map: RefCell<HashMap<RuntimeString, VmValue>>,
    /// Cached `constructor` lookup keyed by vtable_version, so hot `new`
    /// paths skip the method_map hash lookup on every instantiation.
    pub ctor_cache: RefCell<Option<(u32, Option<VmValue>)>>,
    /// VM-owned constructor cache keyed by vtable_version: the VM stores its
    /// resolved closure here (`Rc<VmClosure>` as `Rc<dyn Any>`) so hot `new`
    /// paths skip the `VmValue` box clone + downcast. `Some((v, None))` means
    /// "class has no constructor". varn-types only provides the slot.
    pub ctor_rt_cache: RefCell<Option<CtorRtCacheEntry>>,
    /// Cached instance shape and inline field count. In Varn, classes have
    /// fixed, immutable field layouts once declared.
    pub instance_shape_cache: RefCell<Option<(Rc<super::shape::Shape>, usize)>>,
    /// Static memory layout of instances of this class.
    pub layout: RefCell<Option<Rc<crate::class_layout::ClassLayout>>>,
    /// Declared static type of each instance field, by slot. Comes down with
    /// the declaration; the layout is built from it rather than from a guess.
    pub field_tags: RefCell<Vec<Option<varn_core::RuntimeKind>>>,
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
            method_map: RefCell::new(HashMap::default()),
            statics: RefCell::new(HashMap::default()),
            static_fields: RefCell::new(Vec::new()),
            root_shape: RefCell::new(super::shape::root_shape()),
            getter_map: RefCell::new(HashMap::default()),
            getter_vtable: RefCell::new(Vec::new()),
            getter_vtable_owners: RefCell::new(Vec::new()),
            setter_map: RefCell::new(HashMap::default()),
            setter_vtable: RefCell::new(Vec::new()),
            setter_vtable_owners: RefCell::new(Vec::new()),
            static_getter_map: RefCell::new(HashMap::default()),
            static_setter_map: RefCell::new(HashMap::default()),
            ctor_cache: RefCell::new(None),
            ctor_rt_cache: RefCell::new(None),
            instance_shape_cache: RefCell::new(None),
            layout: RefCell::new(None),
            field_tags: RefCell::new(Vec::new()),
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
        CLASS_REGISTRY.with(|reg| {
            reg.borrow_mut().insert(cls.id, Rc::downgrade(&cls));
        });
        cls
    }

    pub fn new_native_rc(name: impl Into<String>) -> Rc<Self> {
        let cls = Rc::new(Self::new_native(name));
        cls.init_root_shape();
        CLASS_REGISTRY.with(|reg| {
            reg.borrow_mut().insert(cls.id, Rc::downgrade(&cls));
        });
        cls
    }

    pub fn find_by_id(id: u32) -> Option<Rc<Self>> {
        CLASS_REGISTRY.with(|reg| reg.borrow().get(&id).and_then(|w| w.upgrade()))
    }

    pub fn init_root_shape(self: &Rc<Self>) {
        let mut root = self.root_shape.borrow_mut();
        if root.class.is_none() {
            *root = super::shape::Shape::create(Some(self.clone()), HashMap::default());
        }
    }

    pub fn instance_shape(&self) -> (Rc<super::shape::Shape>, usize) {
        if let Some(ref cached) = *self.instance_shape_cache.borrow() {
            return (Rc::clone(&cached.0), cached.1);
        }
        let root = self.root_shape.borrow();
        let n = root.property_names.len();
        let shape = Rc::clone(&*root);
        *self.instance_shape_cache.borrow_mut() = Some((Rc::clone(&shape), n));
        (shape, n)
    }

    pub fn inherit_fields(self: &Rc<Self>, parent: &ClassObj) {
        *self.instance_shape_cache.borrow_mut() = None;
        *self.layout.borrow_mut() = None;
        let mut properties = parent.root_shape.borrow().property_names.clone();
        let mut tags = parent.field_tags.borrow().clone();
        tags.resize(properties.len(), None);
        let own_tags = self.field_tags.borrow().clone();
        let mut own: Vec<(RuntimeString, usize)> = self
            .root_shape
            .borrow()
            .property_names
            .iter()
            .filter(|(name, _)| !properties.contains_key(name.as_ref()))
            .map(|(k, &v)| (k.clone(), v))
            .collect();
        own.sort_unstable_by_key(|(_, slot)| *slot);
        for (name, old_slot) in own {
            properties.insert(name, tags.len());
            tags.push(own_tags.get(old_slot).copied().flatten());
        }
        *self.root_shape.borrow_mut() = super::shape::Shape::create(Some(self.clone()), properties);
        *self.field_tags.borrow_mut() = tags;
    }

    pub fn declare_field(&self, name: RuntimeString, tag: Option<varn_core::RuntimeKind>) -> usize {
        *self.instance_shape_cache.borrow_mut() = None;
        *self.layout.borrow_mut() = None;
        let mut root = self.root_shape.borrow_mut();
        if let Some(&slot) = root.property_names.get(&name) {
            self.field_tags.borrow_mut()[slot] = tag;
            return slot;
        }
        let new_shape = root.extend_property(name);
        let slot = new_shape.property_names.len() - 1;
        *root = new_shape;
        let mut tags = self.field_tags.borrow_mut();
        tags.resize(slot + 1, None);
        tags[slot] = tag;
        slot
    }

    /// Returns the static memory layout of this class, computing it if not yet cached.
    pub fn get_or_compute_layout(&self) -> Rc<crate::class_layout::ClassLayout> {
        if let Some(ref lay) = *self.layout.borrow() {
            return Rc::clone(lay);
        }
        let shape = self.root_shape.borrow();
        let mut ordered: Vec<(usize, RuntimeString)> = shape
            .property_names
            .iter()
            .map(|(k, &slot)| (slot, Arc::clone(k)))
            .collect();
        ordered.sort_unstable_by_key(|(slot, _)| *slot);

        let tags = self.field_tags.borrow();
        let fields_in: Vec<(Arc<str>, Option<varn_core::RuntimeKind>)> = ordered
            .into_iter()
            .map(|(slot, name)| {
                let tag = tags.get(slot).copied().flatten();
                (Arc::from(name.as_ref()), tag)
            })
            .collect();

        let layout = Rc::new(crate::class_layout::ClassLayout::from_fields(
            &*self.name,
            self.id,
            &fields_in,
        ));
        *self.layout.borrow_mut() = Some(Rc::clone(&layout));
        layout
    }

    pub fn for_each_value_mut(&self, f: &mut dyn FnMut(&mut VmValue)) {
        for v in self.vtable.borrow_mut().iter_mut() {
            f(v);
        }
        for v in self.getter_vtable.borrow_mut().iter_mut() {
            f(v);
        }
        for v in self.setter_vtable.borrow_mut().iter_mut() {
            f(v);
        }
        for v in self.statics.borrow_mut().values_mut() {
            f(v);
        }
        for v in self.static_getter_map.borrow_mut().values_mut() {
            f(v);
        }
        for v in self.static_setter_map.borrow_mut().values_mut() {
            f(v);
        }
        *self.ctor_cache.borrow_mut() = None;
    }

    pub fn add_method(&self, name: impl Into<Arc<str>>, value: VmValue) {
        self.add_method_with_owner(name, value, None);
    }

    pub fn add_method_with_owner(
        &self,
        name: impl Into<Arc<str>>,
        value: VmValue,
        owner: Option<Rc<ClassObj>>,
    ) {
        let name: Arc<str> = name.into();
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

    pub fn add_getter(&self, name: impl Into<Arc<str>>, value: VmValue) {
        self.add_getter_with_owner(name, value, None);
    }

    pub fn add_getter_with_owner(
        &self,
        name: impl Into<Arc<str>>,
        value: VmValue,
        owner: Option<Rc<ClassObj>>,
    ) {
        let name: Arc<str> = name.into();
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
        name: impl Into<Arc<str>>,
        value: VmValue,
        owner: Option<Rc<ClassObj>>,
    ) {
        let name: Arc<str> = name.into();
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

    pub fn add_static_getter(&self, name: impl Into<Arc<str>>, value: VmValue) {
        let name_rc = name.into();
        let mut fields = self.static_fields.borrow_mut();
        if !fields.contains(&name_rc) {
            fields.push(name_rc.clone());
        }
        self.static_getter_map.borrow_mut().insert(name_rc, value);
    }

    pub fn add_static_setter(&self, name: impl Into<Arc<str>>, value: VmValue) {
        let name_rc = name.into();
        let mut fields = self.static_fields.borrow_mut();
        if !fields.contains(&name_rc) {
            fields.push(name_rc.clone());
        }
        self.static_setter_map.borrow_mut().insert(name_rc, value);
    }

    pub fn add_static(&self, name: impl Into<Arc<str>>, value: VmValue) {
        let name_rc = name.into();
        let mut fields = self.static_fields.borrow_mut();
        if !fields.contains(&name_rc) {
            fields.push(name_rc.clone());
        }
        self.statics.borrow_mut().insert(name_rc, value);
    }

    pub fn get_static(&self, name: &str) -> Option<VmValue> {
        self.statics.borrow().get(name).copied()
    }

    pub fn find_getter(&self, name: &str) -> Option<VmValue> {
        if let Some(&idx) = self.getter_map.borrow().get(name) {
            return Some(self.getter_vtable.borrow()[idx]);
        }
        self.superclass.borrow().as_ref()?.find_getter(name)
    }

    pub fn find_setter(&self, name: &str) -> Option<VmValue> {
        if let Some(&idx) = self.setter_map.borrow().get(name) {
            return Some(self.setter_vtable.borrow()[idx]);
        }
        self.superclass.borrow().as_ref()?.find_setter(name)
    }

    pub fn find_static_getter(&self, name: &str) -> Option<VmValue> {
        if let Some(v) = self.static_getter_map.borrow().get(name) {
            return Some(*v);
        }
        self.superclass.borrow().as_ref()?.find_static_getter(name)
    }

    pub fn find_static_setter(&self, name: &str) -> Option<VmValue> {
        if let Some(v) = self.static_setter_map.borrow().get(name) {
            return Some(*v);
        }
        self.superclass.borrow().as_ref()?.find_static_setter(name)
    }

    pub fn find_method(&self, name: &str) -> Option<VmValue> {
        if let Some(&idx) = self.method_map.borrow().get(name) {
            return Some(self.vtable.borrow()[idx]);
        }
        if let Some(super_cls) = &*self.superclass.borrow() {
            return super_cls.find_method(name);
        }
        None
    }

    /// Cached `find_method("constructor")`, invalidated by vtable_version.
    pub fn constructor(&self) -> Option<VmValue> {
        let ver = self.vtable_version.load(Ordering::Relaxed);
        if let Some((cached_ver, ctor)) = self.ctor_cache.borrow().as_ref() {
            if *cached_ver == ver {
                return *ctor;
            }
        }
        let ctor = self.find_method("constructor");
        *self.ctor_cache.borrow_mut() = Some((ver, ctor));
        ctor
    }
}

pub fn find_method_with_owner(class: &Rc<ClassObj>, name: &str) -> Option<(VmValue, Rc<ClassObj>)> {
    if let Some(&idx) = class.method_map.borrow().get(name) {
        return Some((class.vtable.borrow()[idx], class.clone()));
    }
    if let Some(super_cls) = &*class.superclass.borrow() {
        return find_method_with_owner(super_cls, name);
    }
    None
}
