use rustc_hash::FxHashMap;

/// Handle a un string interno, deduplicado. `Copy` — comparar dos `Atom` es
/// comparar dos `u32`, nunca contenido de texto.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Atom(u32);

/// How many new entries an `AtomInterner`'s delta may hold before the next
/// `intern` folds it into a fresh frozen base. Bounds the cost of every
/// `Clone` (an `Arc::clone` plus a delta of at most this size) regardless of
/// how many atoms the whole session has interned — see
/// `docs/plans/2026-09-25-shared-atom-type-tables.md` §3 (Enfoque B).
const FREEZE_THRESHOLD: usize = 2048;

/// Frozen, immutable half of an `AtomInterner`. Shared via `Arc`, so cloning
/// an `AtomInterner` never copies this: only the (bounded) delta is copied.
#[derive(Debug, Default)]
struct AtomBase {
    map: FxHashMap<Box<str>, Atom>,
    strings: Vec<Box<str>>,
}

/// Tabla de interning por sesión de compilación completa (no por parseo
/// individual): un módulo importa símbolos de otro, así que sus `Atom`s
/// deben ser comparables entre sí, y por eso viven todos en la misma tabla
/// compartida. `AstArena` NO la contiene -- son dos estructuras separadas,
/// pasadas juntas a quien las necesite.
///
/// Internally split into a frozen `base` (shared via `Arc`, so cloning it is
/// O(1)) and a small `delta` of entries interned since the last freeze.
/// `Atom` indices are positional and monotonically assigned regardless of
/// which half holds an entry, so freezing never changes what index a
/// previously-minted `Atom` resolves to.
#[derive(Debug, Default, Clone)]
pub struct AtomInterner {
    base: std::sync::Arc<AtomBase>,
    delta_map: FxHashMap<Box<str>, Atom>,
    delta_strings: Vec<Box<str>>,
}

impl AtomInterner {
    pub fn new() -> Self {
        Self::default()
    }

    fn base_len(&self) -> usize {
        self.base.strings.len()
    }

    /// Fold `delta` into a fresh frozen `base`, leaving `delta` empty. O(n)
    /// in the total table size, but only runs once every `FREEZE_THRESHOLD`
    /// new atoms, not once per caller.
    fn freeze(&mut self) {
        if self.delta_strings.is_empty() {
            return;
        }
        let mut map = self.base.map.clone();
        let mut strings = self.base.strings.clone();
        for s in self.delta_strings.drain(..) {
            map.insert(s.clone(), Atom(strings.len() as u32));
            strings.push(s);
        }
        self.delta_map.clear();
        self.base = std::sync::Arc::new(AtomBase { map, strings });
    }

    pub fn intern(&mut self, s: &str) -> Atom {
        if let Some(&atom) = self.base.map.get(s) {
            return atom;
        }
        if let Some(&atom) = self.delta_map.get(s) {
            return atom;
        }
        let id = (self.base_len() + self.delta_strings.len()) as u32;
        let boxed: Box<str> = s.into();
        self.delta_map.insert(boxed.clone(), Atom(id));
        self.delta_strings.push(boxed);
        if self.delta_strings.len() >= FREEZE_THRESHOLD {
            self.freeze();
        }
        Atom(id)
    }

    /// Non-mutating lookup: the `Atom` for `s` if it was already interned,
    /// `None` otherwise. Used by `&str`-keyed lookup APIs (e.g.
    /// `BindResult`/`BindView` symbol resolution) that must not intern new
    /// text on a read path — an unresolved name should report "not found",
    /// not silently grow the table with a text that no declaration ever
    /// produced.
    pub fn get(&self, s: &str) -> Option<Atom> {
        self.base
            .map
            .get(s)
            .or_else(|| self.delta_map.get(s))
            .copied()
    }

    /// Panics on an atom this table never minted; the panic names the
    /// caller, which is the site that should have used `try_resolve`.
    #[track_caller]
    pub fn resolve(&self, atom: Atom) -> &str {
        self.try_resolve(atom)
            .unwrap_or_else(|| panic!("Atom {atom:?} is not present in this interner"))
    }

    /// Bounds-checked `resolve`: `None` instead of panicking when `atom` was
    /// interned by a *different* `AtomInterner` than `self` (e.g. a
    /// `ctx`-less call site with only a placeholder interner on hand — see
    /// `resolve_type_node`'s `default_interner` fallback).
    pub fn try_resolve(&self, atom: Atom) -> Option<&str> {
        let idx = atom.0 as usize;
        let base_len = self.base_len();
        if idx < base_len {
            self.base.strings.get(idx).map(|s| s.as_ref())
        } else {
            self.delta_strings.get(idx - base_len).map(|s| s.as_ref())
        }
    }

    pub fn len(&self) -> usize {
        self.base_len() + self.delta_strings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Interned strings in insertion order, i.e. `Atom(0), Atom(1), ...`.
    /// `Atom`'s inner index is private (it is not meant to be reconstructed
    /// by callers), so this is the sanctioned way for a debug-only invariant
    /// check (e.g. `ModuleResolver::set_interner`) to compare two
    /// interners' entries by text without minting `Atom`s of its own.
    pub fn iter_strings(&self) -> impl Iterator<Item = &str> {
        self.base
            .strings
            .iter()
            .chain(self.delta_strings.iter())
            .map(|s| s.as_ref())
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

    #[test]
    fn interning_past_freeze_threshold_still_resolves_correctly() {
        let mut interner = AtomInterner::new();
        let mut atoms = Vec::new();
        for i in 0..(FREEZE_THRESHOLD * 2 + 3) {
            atoms.push(interner.intern(&format!("atom{i}")));
        }
        for (i, atom) in atoms.iter().enumerate() {
            assert_eq!(interner.resolve(*atom), format!("atom{i}"));
        }
        assert_eq!(interner.len(), FREEZE_THRESHOLD * 2 + 3);
    }

    #[test]
    fn delta_stays_bounded_by_freeze_threshold() {
        let mut interner = AtomInterner::new();
        for i in 0..(FREEZE_THRESHOLD * 3 + 7) {
            interner.intern(&format!("bounded{i}"));
        }
        assert!(
            interner.delta_strings.len() < FREEZE_THRESHOLD,
            "delta grew to {} entries, unbounded by FREEZE_THRESHOLD ({FREEZE_THRESHOLD})",
            interner.delta_strings.len()
        );
    }

    #[test]
    fn resolve_at_base_delta_boundary_is_correct() {
        let mut interner = AtomInterner::new();
        for i in 0..(FREEZE_THRESHOLD + 1) {
            interner.intern(&format!("a{i}"));
        }
        // At least one freeze has happened; `a{FREEZE_THRESHOLD}` is in the
        // delta, everything before it is (or was) in a frozen base.
        let last_base_text = format!("a{}", FREEZE_THRESHOLD - 1);
        let first_delta_text = format!("a{FREEZE_THRESHOLD}");
        let last_base = interner.get(&last_base_text).unwrap();
        let first_delta = interner.get(&first_delta_text).unwrap();
        assert_eq!(interner.resolve(last_base), last_base_text);
        assert_eq!(interner.resolve(first_delta), first_delta_text);
    }

    #[test]
    fn try_resolve_out_of_range_after_freeze_is_none() {
        let mut interner = AtomInterner::new();
        for i in 0..(FREEZE_THRESHOLD + 1) {
            interner.intern(&format!("a{i}"));
        }
        let out_of_range = Atom((FREEZE_THRESHOLD as u32) * 100);
        assert_eq!(interner.try_resolve(out_of_range), None);
    }

    #[test]
    fn iter_strings_preserves_insertion_order_across_freeze() {
        let mut interner = AtomInterner::new();
        let mut expected = Vec::new();
        for i in 0..(FREEZE_THRESHOLD + 5) {
            let s = format!("s{i}");
            interner.intern(&s);
            expected.push(s);
        }
        let got: Vec<String> = interner.iter_strings().map(|s| s.to_owned()).collect();
        assert_eq!(got, expected);
    }

    #[test]
    fn clone_after_freeze_does_not_leak_delta_between_instances() {
        let mut interner = AtomInterner::new();
        for i in 0..(FREEZE_THRESHOLD + 1) {
            interner.intern(&format!("a{i}"));
        }
        let mut clone = interner.clone();
        let extra = clone.intern("only-in-clone");
        assert!(interner.get("only-in-clone").is_none());
        assert_eq!(clone.resolve(extra), "only-in-clone");
    }
}
