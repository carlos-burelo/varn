use rustc_hash::FxHashMap;

/// Handle a un string interno, deduplicado. `Copy` — comparar dos `Atom` es
/// comparar dos `u32`, nunca contenido de texto.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Atom(u32);

/// Tabla de interning por sesión de compilación completa (no por parseo
/// individual): un módulo importa símbolos de otro, así que sus `Atom`s
/// deben ser comparables entre sí, y por eso viven todos en la misma tabla
/// compartida. `AstArena` NO la contiene -- son dos estructuras separadas,
/// pasadas juntas a quien las necesite.
#[derive(Debug, Default, Clone)]
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

    /// Non-mutating lookup: the `Atom` for `s` if it was already interned,
    /// `None` otherwise. Used by `&str`-keyed lookup APIs (e.g.
    /// `BindResult`/`BindView` symbol resolution) that must not intern new
    /// text on a read path — an unresolved name should report "not found",
    /// not silently grow the table with a text that no declaration ever
    /// produced.
    pub fn get(&self, s: &str) -> Option<Atom> {
        self.map.get(s).copied()
    }

    pub fn resolve(&self, atom: Atom) -> &str {
        &self.strings[atom.0 as usize]
    }

    /// Bounds-checked `resolve`: `None` instead of panicking when `atom` was
    /// interned by a *different* `AtomInterner` than `self` (e.g. a
    /// `ctx`-less call site with only a placeholder interner on hand — see
    /// `resolve_type_node`'s `default_interner` fallback).
    pub fn try_resolve(&self, atom: Atom) -> Option<&str> {
        self.strings.get(atom.0 as usize).map(|s| s.as_ref())
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    /// Interned strings in insertion order, i.e. `Atom(0), Atom(1), ...`.
    /// `Atom`'s inner index is private (it is not meant to be reconstructed
    /// by callers), so this is the sanctioned way for a debug-only invariant
    /// check (e.g. `ModuleResolver::set_interner`) to compare two
    /// interners' entries by text without minting `Atom`s of its own.
    pub fn iter_strings(&self) -> impl Iterator<Item = &str> {
        self.strings.iter().map(|s| s.as_ref())
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
