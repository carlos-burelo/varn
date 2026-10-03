use super::table::CheckerTyTable;
use varn_core::{Atom, AtomInterner, BuiltinType};

pub(super) fn builtin_names() -> AtomInterner {
    let mut names = AtomInterner::new();
    for builtin in BuiltinType::ALL {
        names.intern(builtin.name());
    }
    names
}

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
