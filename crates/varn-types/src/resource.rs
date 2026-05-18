use std::any::Any;
use std::collections::HashMap;

pub struct ResourceStore {
    next_id: u32,
    map: HashMap<u32, Box<dyn Any>>,
}

impl ResourceStore {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            map: HashMap::new(),
        }
    }

    pub fn insert<T: Any>(&mut self, resource: T) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.map.insert(id, Box::new(resource));
        id
    }

    pub fn get<T: Any>(&self, id: u32) -> Option<&T> {
        self.map.get(&id)?.downcast_ref()
    }

    pub fn get_mut<T: Any>(&mut self, id: u32) -> Option<&mut T> {
        self.map.get_mut(&id)?.downcast_mut()
    }

    pub fn remove<T: Any>(&mut self, id: u32) -> Option<T> {
        let boxed = self.map.remove(&id)?;
        boxed.downcast().ok().map(|b| *b)
    }

    pub fn contains(&self, id: u32) -> bool {
        self.map.contains_key(&id)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl Default for ResourceStore {
    fn default() -> Self {
        Self::new()
    }
}
