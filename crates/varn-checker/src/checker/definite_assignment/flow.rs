use rustc_hash::{FxHashMap, FxHashSet};
use varn_core::Atom;

#[derive(Clone, Default)]
pub(super) struct Flow {
    pub(super) pending: FxHashSet<Atom>,

    pub(super) assigned: FxHashSet<Atom>,

    pub(super) undeclared: FxHashSet<Atom>,

    pub(super) callees: FxHashMap<Atom, FxHashSet<Atom>>,

    pub(super) diverged: bool,
}

pub(super) struct Hoisted {
    pub(super) name: Atom,
    pub(super) is_binding: bool,
    pub(super) reads: Option<FxHashSet<Atom>>,
}

pub(super) struct Shadowed {
    name: Atom,
    pending: bool,
    assigned: bool,
    undeclared: bool,
    callee: Option<FxHashSet<Atom>>,
}

impl Flow {
    pub(super) fn assign(&mut self, name: &Atom) {
        self.assigned.insert(*name);
    }

    pub(super) fn declare(&mut self, name: &Atom, initialized: bool) {
        self.undeclared.remove(name);
        if initialized {
            self.assigned.insert(*name);
            self.pending.remove(name);
        } else {
            self.pending.insert(*name);
            self.assigned.remove(name);
        }
    }

    pub(super) fn unready(&self, name: &Atom) -> bool {
        self.undeclared.contains(name)
            || (self.pending.contains(name) && !self.assigned.contains(name))
    }

    pub(super) fn enter(&mut self, hoisted: Vec<Hoisted>) -> Vec<Shadowed> {
        let mut saved = Vec::with_capacity(hoisted.len());
        for h in hoisted {
            saved.push(Shadowed {
                name: h.name,
                pending: self.pending.contains(&h.name),
                assigned: self.assigned.contains(&h.name),
                undeclared: self.undeclared.contains(&h.name),
                callee: self.callees.remove(&h.name),
            });
            self.assigned.remove(&h.name);
            if h.is_binding {
                self.pending.insert(h.name);
                self.undeclared.insert(h.name);
            } else {
                self.pending.remove(&h.name);
                self.undeclared.remove(&h.name);
            }
            if let Some(reads) = h.reads {
                self.callees.insert(h.name, reads);
            }
        }
        saved
    }

    pub(super) fn leave(&mut self, saved: Vec<Shadowed>) {
        for s in saved.into_iter().rev() {
            restore(&mut self.pending, s.name, s.pending);
            restore(&mut self.assigned, s.name, s.assigned);
            restore(&mut self.undeclared, s.name, s.undeclared);
            self.callees.remove(&s.name);
            if let Some(reads) = s.callee {
                self.callees.insert(s.name, reads);
            }
        }
    }

    pub(super) fn callee_reads(&self, name: &Atom) -> Option<FxHashSet<Atom>> {
        let direct = self.callees.get(name)?;
        let mut closure = direct.clone();
        let mut frontier: Vec<Atom> = direct.iter().copied().collect();
        while let Some(next) = frontier.pop() {
            if let Some(reads) = self.callees.get(&next) {
                for r in reads {
                    if closure.insert(*r) {
                        frontier.push(*r);
                    }
                }
            }
        }
        Some(closure)
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

fn restore(set: &mut FxHashSet<Atom>, name: Atom, was_member: bool) {
    if was_member {
        set.insert(name);
    } else {
        set.remove(&name);
    }
}
