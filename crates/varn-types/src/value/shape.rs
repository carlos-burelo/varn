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
    /// [`Self::property_names`] in slot order: `ordered[i]` names slot `i`.
    ordered: Vec<RuntimeString>,
    /// Pre-rendered JSON property prefix for slot `i` (e.g. `"\"id\":"` for slot 0, `",\"name\":"` for slot 1).
    json_prefixes: Vec<String>,
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
        let mut by_slot: Vec<(usize, RuntimeString)> = property_names
            .iter()
            .map(|(k, &slot)| (slot, Rc::clone(k)))
            .collect();
        by_slot.sort_unstable_by_key(|(slot, _)| *slot);
        let ordered: Vec<RuntimeString> = by_slot.into_iter().map(|(_, k)| k).collect();
        let mut json_prefixes = Vec::with_capacity(ordered.len());
        for (i, name) in ordered.iter().enumerate() {
            let mut p = String::new();
            if i > 0 {
                p.push(',');
            }
            p.push('"');
            for b in name.as_bytes() {
                match b {
                    b'"' => p.push_str("\\\""),
                    b'\\' => p.push_str("\\\\"),
                    _ => p.push(*b as char),
                }
            }
            p.push_str("\":");
            json_prefixes.push(p);
        }
        Rc::new(Shape {
            id: NEXT_SHAPE_ID.fetch_add(1, Ordering::Relaxed),
            class,
            property_names,
            ordered,
            json_prefixes,
            transitions: RefCell::new(HashMap::new()),
        })
    }

    /// The property names in slot order. `ordered_names()[i]` is the name of
    /// the field `ObjRef::field_at(i)` returns.
    #[inline]
    pub fn ordered_names(&self) -> &[RuntimeString] {
        &self.ordered
    }

    #[inline]
    pub fn json_prefixes(&self) -> &[String] {
        &self.json_prefixes
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
