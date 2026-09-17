use rustc_hash::FxHashMap;

/// Handle a un string interno, deduplicado. `Copy` — comparar dos `Atom` es
/// comparar dos `u32`, nunca contenido de texto.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Atom(u32);

/// Tabla de interning por sesión de compilación. Una instancia vive por
/// parseo (ver `AstArena`, que la contiene desde el Componente 2).
#[derive(Debug, Default)]
pub struct AtomInterner {
    map: FxHashMap<Box<str>, Atom>,
    strings: Vec<Box<str>>,
}

impl AtomInterner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, s: &str) -> Atom {
        if let Some(&atom) = self.map.get(s) {
            return atom;
        }
        let id = self.strings.len() as u32;
        let boxed: Box<str> = s.into();
        self.map.insert(boxed.clone(), Atom(id));
        self.strings.push(boxed);
        Atom(id)
    }

    pub fn resolve(&self, atom: Atom) -> &str {
        &self.strings[atom.0 as usize]
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_same_string_twice_returns_same_atom() {
        let mut interner = AtomInterner::new();
        let a1 = interner.intern("foo");
        let a2 = interner.intern("foo");
        assert_eq!(a1, a2);
    }

    #[test]
    fn interning_different_strings_returns_different_atoms() {
        let mut interner = AtomInterner::new();
        let a1 = interner.intern("foo");
        let a2 = interner.intern("bar");
        assert_ne!(a1, a2);
    }

    #[test]
    fn resolve_roundtrips_the_original_text() {
        let mut interner = AtomInterner::new();
        let a = interner.intern("hello world");
        assert_eq!(interner.resolve(a), "hello world");
    }

    #[test]
    fn empty_string_interns_like_any_other() {
        let mut interner = AtomInterner::new();
        let a = interner.intern("");
        assert_eq!(interner.resolve(a), "");
        assert_eq!(interner.intern(""), a);
    }

    #[test]
    fn len_counts_distinct_strings_only() {
        let mut interner = AtomInterner::new();
        interner.intern("a");
        interner.intern("b");
        interner.intern("a");
        assert_eq!(interner.len(), 2);
    }
}
