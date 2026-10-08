use super::cells::{CellSpace, SlotState};
use super::children::{for_each_child, Reach};
use super::structs::HeapInner;
use crate::value::VmValue;
use varn_types::HeapRef;

impl HeapInner {
    pub(crate) fn minor_gc(&mut self, roots: &[HeapRef]) {
        let trace = crate::gc_trace::note_start(self.young.len(), self.young.minor_gc_promoted);
        self.young.minor_gc_count += 1;
        let mut work = std::mem::take(&mut self.young.worklist);
        work.clear();

        for &root in roots {
            CellSpace::mark_young(root, &mut work);
        }

        let mut remembered = std::mem::take(&mut self.young.remembered);
        for &r in &remembered {
            if self.cells.state(r) == SlotState::Remembered {
                self.cells.set_state(r, SlotState::Old);
            }
            self.mark_children(r, &mut work);
        }
        remembered.clear();
        self.young.remembered = remembered;

        let scan_roots = std::mem::take(&mut self.scan_roots);
        for &r in &scan_roots {
            self.mark_children(r, &mut work);
        }
        self.scan_roots = scan_roots;

        for cell in std::mem::take(&mut self.young_cells) {
            cell.trace_cells(&mut |c| mark_young_value(c.get(), &mut work));
        }
        for lazy in std::mem::take(&mut self.young_lazies) {
            lazy.trace_cells(&mut |c| mark_young_value(c.get(), &mut work));
        }

        while let Some(r) = work.pop() {
            self.mark_children(r, &mut work);
        }
        self.young.worklist = work;

        self.sweep_young();
        crate::gc_trace::note_end(
            trace,
            self.young.minor_gc_count,
            self.young.minor_gc_promoted,
        );
    }

    fn mark_children(&self, r: HeapRef, work: &mut Vec<HeapRef>) {
        for_each_child(
            &self.cells,
            r,
            &self.identity_index,
            Reach::Minor,
            &mut |child| CellSpace::mark_young(child, work),
        );
    }

    fn sweep_young(&mut self) {
        let mut born = std::mem::take(&mut self.young.born);
        for &r in &born {
            match self.cells.state(r) {
                SlotState::Marked => {
                    self.cells.promote(r);
                    self.young.minor_gc_promoted += 1;
                    let (track, identity) = match self.instance(r) {
                        Some(_) => (false, None),
                        None => {
                            let obj = self.cells.get(r);
                            (Self::needs_minor_scan(obj), Self::identity_key(obj))
                        }
                    };
                    if track {
                        self.scan_roots.push(r);
                    }
                    if let Some(key) = identity {
                        self.identity_index.insert(key, r);
                    }
                }
                SlotState::Young => self.cells.release(r),
                SlotState::Old | SlotState::Remembered | SlotState::Free => {}
            }
        }
        self.young.retired += born.len() as u64;
        born.clear();
        self.young.born = born;
    }
}

#[inline]
fn mark_young_value(v: VmValue, work: &mut Vec<HeapRef>) {
    if v.is_heap() {
        CellSpace::mark_young(v.as_heap(), work);
    }
}

#[cfg(test)]
mod minor_gc_tests {
    use super::super::structs::HeapInner;
    use varn_types::ClassObj;

    fn test_heap_with_class() -> (crate::heap::Heap, std::rc::Rc<ClassObj>) {
        let heap = crate::heap::Heap::new();
        let cls = ClassObj::new_rc("Probe");
        let layout = std::rc::Rc::new(varn_core::layout::ClassLayout::from_fields(&[(
            std::sync::Arc::from("x"),
            Some(varn_core::RuntimeKind::Int),
        )]));
        cls.set_layout(layout);
        (heap, cls)
    }

    #[test]
    fn minor_gc_with_colocated_instances() {
        let (mut heap, cls) = test_heap_with_class();
        let inner = unsafe { heap.inner_mut() };
        let mut refs = Vec::new();
        for _ in 0..100 {
            let (r, inst) = inner.alloc_instance(&cls);
            assert_eq!(inst.class_id, cls.id);
            refs.push(r);
        }
        assert_eq!(inner.instance(refs[3]).unwrap().class_id, cls.id);
        inner.minor_gc(&refs);
        for r in &refs {
            assert_eq!(inner.instance(*r).unwrap().class_id, cls.id);
        }
    }

    #[test]
    fn minor_gc_with_many_colocated_instances() {
        let (mut heap, cls) = test_heap_with_class();
        let inner = unsafe { heap.inner_mut() };
        let mut refs = Vec::with_capacity(60000);
        for _ in 0..60000 {
            let (r, _) = inner.alloc_instance(&cls);
            refs.push(r);
        }
        inner.minor_gc(&refs);
        for r in refs.iter().step_by(997) {
            assert_eq!(inner.instance(*r).unwrap().class_id, cls.id);
        }
    }

    #[test]
    fn minor_gc_with_array_root_of_instances() {
        use varn_types::VmValue;
        let (mut heap, cls) = test_heap_with_class();
        let inner = unsafe { heap.inner_mut() };
        let mut vals = Vec::with_capacity(60000);
        for _ in 0..60000 {
            let (r, _) = inner.alloc_instance(&cls);
            vals.push(VmValue::from_heap(r));
        }
        let arr = inner.alloc_array_vm(vals);
        let arr_ref = arr.as_heap();
        inner.minor_gc(&[arr_ref]);
        let expected = cls.id;
        match inner.cells.get(arr_ref) {
            crate::heap::HeapObj::Array(a) => {
                let items = match a.repr() {
                    varn_types::ArrayRepr::Boxed(v) => v.as_vec(),
                    _ => panic!("array repr cambio"),
                };
                assert_eq!(items.len(), 60000);
                let first = inner
                    .instance(items[0].as_heap())
                    .expect("instance survives");
                assert_eq!(first.class_id, expected);
            }
            other => panic!("array sobrevivio como {other:?}"),
        }
    }
}
