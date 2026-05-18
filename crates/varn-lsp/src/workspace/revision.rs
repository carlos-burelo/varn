use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Debug, Default)]
pub struct Revision(u32);

impl Revision {
    pub fn new() -> Self {
        Self(0)
    }

    pub fn bump(&mut self) -> u32 {
        self.0 += 1;
        self.0
    }

    pub fn current(&self) -> u32 {
        self.0
    }
}

pub struct Cached<T> {
    pub value: T,
    pub input_hash: u64,
}

impl<T> Cached<T> {
    pub fn new(value: T, source: &str) -> Self {
        Self {
            value,
            input_hash: hash_str(source),
        }
    }

    pub fn is_valid_for(&self, source: &str) -> bool {
        self.input_hash == hash_str(source)
    }
}

fn hash_str(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}
