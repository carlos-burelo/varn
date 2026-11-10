use super::cells::{CellSpace, SlotState};
use super::children::{for_each_child, Reach};
use super::structs::HeapInner;
use rustc_hash::FxHashSet;

const MAX_REPORTED: usize = 24;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Minor,
    Major,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Minor => "minor",
            Self::Major => "major",
        }
    }
}

impl HeapInner {
    pub(crate) fn verify_heap(&self, phase: Phase) {
        if !self.verify_gc {
            return;
        }
        let problems = self.heap_problems(phase);
        if problems.is_empty() {
            return;
        }
        let mut message = format!(
            "GC verifier: the heap broke {} invariant(s) after a {} collection",
            problems.len(),
            phase.name()
        );
        for problem in problems.iter().take(MAX_REPORTED) {
            message.push_str("\n  - ");
            message.push_str(problem);
        }
        panic!("{message}");
    }

    fn heap_problems(&self, phase: Phase) -> Vec<String> {
        let mut problems = Vec::new();
        let mut live = 0usize;
        let mut young = 0usize;
        for r in self.cells.refs() {
            live += 1;
            let state = self.cells.state(r);
            if CellSpace::is_marked(r) {
                problems.push(format!("cell {r:?} keeps the major mark"));
            }
            if state == SlotState::Marked {
                problems.push(format!("cell {r:?} keeps the minor mark"));
            }
            if state == SlotState::Young {
                young += 1;
            }
            for_each_child(
                &self.cells,
                r,
                &self.identity_index,
                Reach::Major,
                &mut |child| {
                    let child_state = self.cells.state(child);
                    if child_state == SlotState::Free {
                        problems.push(format!("live cell {r:?} points to freed cell {child:?}"));
                    } else if phase == Phase::Minor
                        && matches!(state, SlotState::Old | SlotState::Remembered)
                        && matches!(child_state, SlotState::Young | SlotState::Marked)
                    {
                        problems.push(format!(
                            "old cell {r:?} reaches young cell {child:?} with no barrier"
                        ));
                    }
                },
            );
        }
        if phase == Phase::Major && young > 0 {
            problems.push(format!("{young} young cells outlived a major collection"));
        }
        self.free_space_problems(live, &mut problems);
        self.tracking_problems(&mut problems);
        problems
    }

    fn free_space_problems(&self, live: usize, problems: &mut Vec<String>) {
        let free = self.cells.free_list_problems(problems);
        let used = self.cells.used_cells();
        if live + free != used {
            problems.push(format!(
                "counters disagree: {live} live + {free} free != {used} used cells"
            ));
        }
    }

    fn tracking_problems(&self, problems: &mut Vec<String>) {
        let mut scan_roots = FxHashSet::default();
        for &r in &self.scan_roots {
            if !scan_roots.insert(r) {
                problems.push(format!("scan_roots lists {r:?} twice"));
            }
            if self.cells.state(r) == SlotState::Free {
                problems.push(format!("scan_roots lists freed cell {r:?}"));
            }
        }
        for (&key, &r) in &self.identity_index {
            if self.cells.state(r) == SlotState::Free {
                problems.push(format!("identity index maps {key:#x} to freed cell {r:?}"));
            } else if self.cells.header_kind(r) == varn_types::cell::CELL_KIND_INSTANCE
                || Self::identity_key(self.cells.get(r)) != Some(key)
            {
                problems.push(format!(
                    "identity index maps {key:#x} to the wrong cell {r:?}"
                ));
            }
        }
    }
}
