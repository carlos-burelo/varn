use super::cells::{CellSpace, SlotState};
use super::children::for_each_child;
use super::structs::HeapInner;

const MAX_REPORTED: usize = 24;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Minor,
    Major,
}

fn describe_kind(kind: u8) -> &'static str {
    if kind == vn_types::cell::CELL_KIND_INSTANCE {
        return "instance";
    }
    match kind {
        0 => "string",
        1 => "array",
        2 => "tuple",
        3 => "object",
        4 => "buffer",
        5 => "module",
        6 => "frozen module",
        7 => "closure",
        8 => "class",
        9 => "native fn",
        10 => "bound method",
        11 => "map",
        12 => "set",
        13 => "task",
        14 => "task handle",
        15 => "range",
        16 => "enum variant",
        17 => "bigint",
        18 => "decimal",
        19 => "char",
        20 => "generator",
        21 => "spread",
        _ => "unknown",
    }
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
    pub(crate) fn verify_heap(&mut self, phase: Phase, roots: &[vn_types::HeapRef]) {
        if !self.verify_gc {
            return;
        }
        let problems = self.heap_problems(phase, roots);
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

    fn heap_problems(&mut self, phase: Phase, roots: &[vn_types::HeapRef]) -> Vec<String> {
        if self.verify_gc {
            self.cells.force_sweep();
            self.forget_swept();
            let cells = &self.cells;
            self.identity_index
                .retain(|_, r| cells.state(*r) != SlotState::Free);
        }
        let mut problems = Vec::new();
        for (index, root) in roots.iter().enumerate() {
            if self.cells.state(*root) == SlotState::Free {
                problems.push(format!("root {index} {root:?} points to a freed cell"));
            }
        }
        let mut live = 0usize;
        let mut young = 0usize;
        for r in self.cells.refs() {
            live += 1;
            let state = self.cells.state(r);
            if self.cells.is_marked(r) {
                problems.push(format!("cell {r:?} keeps the major mark"));
            }
            if CellSpace::is_minor_marked(r) {
                problems.push(format!("cell {r:?} keeps the minor mark"));
            }
            if state == SlotState::Young {
                problems.push(format!(
                    "young {} {r:?} survived a minor collection unmarked (raw {:#x})",
                    describe_kind(self.cells.header_kind(r)),
                    CellSpace::raw_state(r)
                ));
            }
            if state == SlotState::Young || state == SlotState::Survivor {
                young += 1;
            }
            for_each_child(&self.cells, r, &self.identity_index, None, &mut |child| {
                let child_state = self.cells.state(child);
                if child_state == SlotState::Free {
                    problems.push(format!(
                        "live {} `{}` {r:?} (state {}) points to freed {} {child:?} [remembered={} edge={}]",
                        describe_kind(self.cells.header_kind(r)),
                        self.describe_name(r),
                        state as u8,
                        describe_kind(self.cells.header_kind(child)),
                        self.young.remembered.contains(&r),
                        self.young.edges.iter().any(|&(p, _)| p == r)
                    ));
                }
            });
        }
        if phase == Phase::Major && young > 0 {
            problems.push(format!("{young} young cells outlived a major collection"));
        }
        if phase == Phase::Minor {
            for r in self.cells.refs() {
                if self.cells.state(r) == SlotState::Young {
                    problems.push(format!(
                        "young cell {r:?} survived a minor collection unmarked"
                    ));
                }
            }
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

    fn describe_name(&self, r: vn_types::HeapRef) -> String {
        if self.cells.header_kind(r) == vn_types::cell::CELL_KIND_INSTANCE {
            return String::new();
        }
        match self.cells.get(r) {
            super::obj::HeapObj::Class(c) => c.name.to_string(),
            super::obj::HeapObj::Str(_)
            | super::obj::HeapObj::Array(_)
            | super::obj::HeapObj::Tuple(_)
            | super::obj::HeapObj::Object(_)
            | super::obj::HeapObj::Buffer(_)
            | super::obj::HeapObj::Module(_)
            | super::obj::HeapObj::FrozenModule(_)
            | super::obj::HeapObj::VmClosure(_)
            | super::obj::HeapObj::NativeFn(..)
            | super::obj::HeapObj::BoundMethod(_)
            | super::obj::HeapObj::Map(_)
            | super::obj::HeapObj::Set(_)
            | super::obj::HeapObj::Task(_)
            | super::obj::HeapObj::TaskHandle(_)
            | super::obj::HeapObj::Range(_)
            | super::obj::HeapObj::EnumVariant(_)
            | super::obj::HeapObj::BigInt(_)
            | super::obj::HeapObj::Decimal(_)
            | super::obj::HeapObj::Char(_)
            | super::obj::HeapObj::Generator(_)
            | super::obj::HeapObj::Spread(_) => String::new(),
        }
    }

    fn tracking_problems(&self, problems: &mut Vec<String>) {
        for &r in &self.young.remembered {
            if self.cells.state(r) == SlotState::Free {
                problems.push(format!("the remembered set lists freed cell {r:?}"));
            }
        }
        for &(parent, child) in &self.young.edges {
            if self.cells.state(parent) == SlotState::Free {
                problems.push(format!("an edge hangs from freed cell {parent:?}"));
            }
            if self.cells.state(child) == SlotState::Free {
                problems.push(format!(
                    "the edge record keeps child {child:?}, which is free"
                ));
            }
        }
        for (&key, &r) in &self.identity_index {
            if self.cells.state(r) == SlotState::Free {
                problems.push(format!("identity index maps {key:#x} to freed cell {r:?}"));
            } else if self.cells.header_kind(r) == vn_types::cell::CELL_KIND_INSTANCE
                || Self::identity_key(self.cells.get(r)) != Some(key)
            {
                problems.push(format!(
                    "identity index maps {key:#x} to the wrong cell {r:?}"
                ));
            }
        }
    }
}
