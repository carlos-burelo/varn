use super::table::CheckerTyTable;
use varn_core::{Atom, AtomInterner};

impl CheckerTyTable {
    pub fn intern_name(&mut self, text: &str) -> Atom {
        self.names.intern(text)
    }

    pub fn name(&self, atom: Atom) -> Option<&str> {
        self.names.try_resolve(atom)
    }

    pub fn names(&self) -> &AtomInterner {
        &self.names
    }

    pub fn absorb_names(&mut self, names: &AtomInterner) {
        self.names.absorb(names);
    }
}
