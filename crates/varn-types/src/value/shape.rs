use super::RuntimeString;
use crate::vm_value::VmValue;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_SHAPE_ID: AtomicU32 = AtomicU32::new(1);

pub struct Shape {
    pub id: u32,
    pub class: Option<Rc<crate::value::ClassObj>>,
    pub property_names: HashMap<RuntimeString, usize>,
    transitions: RefCell<HashMap<RuntimeString, Rc<Shape>>>,
}

impl std::fmt::Debug for Shape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shape")
            .field("id", &self.id)
            .field("class", &self.class.as_ref().map(|c| &c.name))
            .field("property_names", &self.property_names)
            .finish()
    }
}

impl Shape {
    pub fn create(
        class: Option<Rc<crate::value::ClassObj>>,
        property_names: HashMap<RuntimeString, usize>,
    ) -> Rc<Self> {
        Rc::new(Shape {
            id: NEXT_SHAPE_ID.fetch_add(1, Ordering::Relaxed),
            class,
            property_names,
            transitions: RefCell::new(HashMap::new()),
        })
    }

    pub fn create_root() -> Rc<Self> {
        Shape::create(None, HashMap::new())
    }

    pub fn transition(&self, key: RuntimeString) -> Rc<Shape> {
        let mut trans = self.transitions.borrow_mut();
        if let Some(child) = trans.get(&key) {
            return Rc::clone(child);
        }
        let mut new_props = self.property_names.clone();
        let slot = new_props.len();
        new_props.insert(Rc::clone(&key), slot);
        let child = Shape::create(self.class.clone(), new_props);
        trans.insert(key, Rc::clone(&child));
        child
    }

    pub fn with_class(&self, class: Option<Rc<crate::value::ClassObj>>) -> Rc<Shape> {
        Shape::create(class, self.property_names.clone())
    }
}

thread_local! {
    static ROOT_SHAPE: Rc<Shape> = Shape::create_root();
}

pub fn root_shape() -> Rc<Shape> {
    ROOT_SHAPE.with(|r| Rc::clone(r))
}

#[derive(Debug, Clone)]
pub struct RuntimeObject {
    pub shape: Rc<Shape>,
    pub values: Vec<VmValue>,
}

impl Default for RuntimeObject {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeObject {
    pub fn new() -> Self {
        Self {
            shape: root_shape(),
            values: Vec::new(),
        }
    }

    pub fn with_class(class: Rc<crate::value::ClassObj>) -> Self {
        let shape = class.root_shape.borrow().clone();
        let field_count = shape.property_names.len();
        Self {
            shape,
            values: vec![VmValue::null(); field_count],
        }
    }

    pub fn with_shape(shape: Rc<Shape>, values: Vec<VmValue>) -> Self {
        Self { shape, values }
    }

    #[inline]
    pub fn get(&self, name: &str) -> Option<VmValue> {
        let idx = self.shape.property_names.get(name).copied()?;
        self.values.get(idx).copied()
    }

    pub fn insert(&mut self, name: RuntimeString, value: VmValue) {
        if let Some(&idx) = self.shape.property_names.get(&name) {
            self.values[idx] = value;
        } else {
            self.shape = self.shape.transition(Rc::clone(&name));
            self.values.push(value);
        }
    }

    pub fn remove(&mut self, name: &str) -> Option<VmValue> {
        let removed_slot = *self.shape.property_names.get(name)?;
        let removed_val = self.values[removed_slot];

        let mut remaining: Vec<(RuntimeString, VmValue)> = self
            .shape
            .property_names
            .iter()
            .filter(|(k, _)| k.as_ref() != name)
            .map(|(k, &slot)| (Rc::clone(k), self.values[slot]))
            .collect();
        remaining.sort_by_key(|(k, _)| self.shape.property_names[k]);

        let mut new_shape = root_shape();
        let mut new_values = Vec::with_capacity(remaining.len());
        for (k, v) in remaining {
            new_shape = new_shape.transition(Rc::clone(&k));
            new_values.push(v);
        }
        self.shape = new_shape;
        self.values = new_values;
        Some(removed_val)
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.shape.property_names.contains_key(name)
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn keys(&self) -> std::vec::IntoIter<RuntimeString> {
        let mut pairs: Vec<(RuntimeString, usize)> = self
            .shape
            .property_names
            .iter()
            .map(|(k, &idx)| (k.clone(), idx))
            .collect();
        pairs.sort_unstable_by_key(|(_, idx)| *idx);
        pairs
            .into_iter()
            .map(|(k, _)| k)
            .collect::<Vec<_>>()
            .into_iter()
    }

    pub fn iter(&self) -> std::vec::IntoIter<(RuntimeString, VmValue)> {
        let mut pairs: Vec<(RuntimeString, VmValue, usize)> = self
            .shape
            .property_names
            .iter()
            .map(|(k, &idx)| (k.clone(), self.values[idx], idx))
            .collect();
        pairs.sort_unstable_by_key(|(_, _, idx)| *idx);
        pairs
            .into_iter()
            .map(|(k, v, _)| (k, v))
            .collect::<Vec<_>>()
            .into_iter()
    }
}

impl PartialEq for RuntimeObject {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        for (k, v) in self.iter() {
            match other.get(&k) {
                Some(ov) if ov == v => {}
                _ => return false,
            }
        }
        true
    }
}
impl Eq for RuntimeObject {}
