use rustc_hash::FxHashSet;
use varn_core::Atom;

#[derive(Clone, Default)]
pub(super) struct Flow {
    pub(super) pending: FxHashSet<Atom>,

    pub(super) assigned: FxHashSet<Atom>,

    pub(super) diverged: bool,
}

impl Flow {
    pub(super) fn assign(&mut self, name: &Atom) {
        self.assigned.insert(*name);
    }

    pub(super) fn merge(&mut self, a: Flow, b: Flow) {
        match (a.diverged, b.diverged) {
            (true, true) => self.diverged = true,
            (true, false) => *self = b,
            (false, true) => *self = a,
            (false, false) => {
                let both: FxHashSet<Atom> = a.assigned.intersection(&b.assigned).cloned().collect();
                self.assigned.extend(both);
            }
        }
    }
}
