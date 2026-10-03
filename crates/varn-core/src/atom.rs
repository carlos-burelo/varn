use rustc_hash::FxHashMap;
use std::sync::Arc;

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct Atom(u128);

impl Atom {
    pub fn of(text: &str) -> Atom {
        Atom(xxhash_rust::xxh3::xxh3_128(text.as_bytes()))
    }
}

const FREEZE_THRESHOLD: usize = 2048;

#[derive(Debug, Clone, Default)]
pub struct AtomInterner {
    base: Arc<FxHashMap<Atom, Box<str>>>,
    delta: FxHashMap<Atom, Box<str>>,
    collisions: Vec<(Box<str>, Box<str>)>,
}

impl AtomInterner {
    pub fn new() -> Self {
        Self::default()
    }

    fn lookup(&self, atom: Atom) -> Option<&str> {
        self.base
            .get(&atom)
            .or_else(|| self.delta.get(&atom))
            .map(|s| s.as_ref())
    }

    fn freeze(&mut self) {
        if self.delta.is_empty() {
            return;
        }
        let mut base = (*self.base).clone();
        base.extend(self.delta.drain());
        self.base = Arc::new(base);
    }

    fn insert(&mut self, atom: Atom, text: &str) {
        match self.lookup(atom) {
            Some(existing) if existing == text => {}
            Some(existing) => {
                let pair = (Box::from(existing), Box::from(text));
                self.collisions.push(pair);
            }
            None => {
                self.delta.insert(atom, Box::from(text));
                if self.delta.len() >= FREEZE_THRESHOLD {
                    self.freeze();
                }
            }
        }
    }

    pub fn intern(&mut self, s: &str) -> Atom {
        let atom = Atom::of(s);
        self.insert(atom, s);
        atom
    }

    pub fn get(&self, s: &str) -> Option<Atom> {
        let atom = Atom::of(s);
        (self.lookup(atom) == Some(s)).then_some(atom)
    }

    #[track_caller]
    pub fn resolve(&self, atom: Atom) -> &str {
        self.try_resolve(atom)
            .unwrap_or_else(|| panic!("Atom {atom:?} is not present in this interner"))
    }

    pub fn try_resolve(&self, atom: Atom) -> Option<&str> {
        self.lookup(atom)
    }

    pub fn absorb(&mut self, other: &AtomInterner) {
        if Arc::ptr_eq(&self.base, &other.base) && other.delta.is_empty() {
            return;
        }
        if self.is_empty() {
            self.base = Arc::clone(&other.base);
            self.delta = other.delta.clone();
        } else {
            let shared_base = Arc::ptr_eq(&self.base, &other.base);
            let base_entries = (!shared_base)
                .then(|| other.base.iter())
                .into_iter()
                .flatten();
            let entries: Vec<(Atom, Box<str>)> = base_entries
                .chain(other.delta.iter())
                .filter(|(atom, text)| self.lookup(**atom) != Some(text.as_ref()))
                .map(|(atom, text)| (*atom, text.clone()))
                .collect();
            for (atom, text) in entries {
                self.insert(atom, &text);
            }
        }
        self.collisions.extend(other.collisions.iter().cloned());
    }

    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.base
            .values()
            .chain(self.delta.values())
            .map(|s| s.as_ref())
    }

    pub fn collisions(&self) -> &[(Box<str>, Box<str>)] {
        &self.collisions
    }

    pub fn len(&self) -> usize {
        self.base.len() + self.delta.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
