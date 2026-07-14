use super::RuntimeString;
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
