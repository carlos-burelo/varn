use crate::heap::{HeapInner, HeapObj};
use crate::nursery::{is_old_idx, old_idx_raw};
use std::collections::VecDeque;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkColor {
    White = 0,
    Gray = 1,
    Black = 2,
}

#[derive(Debug, Clone)]
pub enum GcError {
    StackOverflow,
    AllocationFailed,
    InternalError(String),
}

impl fmt::Display for GcError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            GcError::StackOverflow => write!(f, "GC: Stack overflow during mark phase"),
            GcError::AllocationFailed => write!(f, "GC: Allocation failed"),
            GcError::InternalError(msg) => write!(f, "GC: Internal error: {}", msg),
        }
    }
}

impl std::error::Error for GcError {}

#[derive(Clone)]
pub struct MarkBitmap {
    bitmap: Vec<u8>,
}

impl MarkBitmap {
    pub fn new(capacity: u32) -> Self {
        let bytes_needed = ((capacity as usize * 2) + 7) / 8;
        Self {
            bitmap: vec![0u8; bytes_needed],
        }
    }

    #[inline]
    pub fn set_color(&mut self, idx: u32, color: MarkColor) {
        let color_val = color as u8;
        let bit_idx = (idx as usize) * 2;
        let byte_idx = bit_idx / 8;
        let bit_offset = bit_idx % 8;

        if byte_idx >= self.bitmap.len() {
            self.bitmap.resize(byte_idx + 1, 0);
        }

        let byte = &mut self.bitmap[byte_idx];
        let mask = 0x3u8 << bit_offset;
        *byte = (*byte & !mask) | ((color_val & 0x3) << bit_offset);
    }

    #[inline]
    pub fn get_color(&self, idx: u32) -> MarkColor {
        let bit_idx = (idx as usize) * 2;
        let byte_idx = bit_idx / 8;
        let bit_offset = bit_idx % 8;

        if byte_idx >= self.bitmap.len() {
            return MarkColor::White;
        }

        let byte = self.bitmap[byte_idx];
        let color_val = (byte >> bit_offset) & 0x3;

        match color_val {
            0 => MarkColor::White,
            1 => MarkColor::Gray,
            2 => MarkColor::Black,
            _ => MarkColor::White,
        }
    }

    pub fn clear(&mut self) {
        self.bitmap.iter_mut().for_each(|b| *b = 0);
    }
}

#[derive(Clone)]
pub struct TricolorMarker {
    marks: MarkBitmap,
    gray_queue: VecDeque<u32>,
    marked_count: usize,
}

impl TricolorMarker {
    pub fn new(heap_capacity: u32) -> Self {
        Self {
            marks: MarkBitmap::new(heap_capacity),
            gray_queue: VecDeque::new(),
            marked_count: 0,
        }
    }

    pub fn mark_gray(&mut self, idx: u32) {
        let raw = if is_old_idx(idx) {
            old_idx_raw(idx)
        } else {
            idx
        };
        let current = self.marks.get_color(raw);
        if current == MarkColor::White {
            self.marks.set_color(raw, MarkColor::Gray);
            self.gray_queue.push_back(raw);
            self.marked_count += 1;
        }
    }

    pub fn mark_black(&mut self, idx: u32) {
        self.marks.set_color(idx, MarkColor::Black);
    }

    pub fn get_color(&self, idx: u32) -> MarkColor {
        self.marks.get_color(idx)
    }

    pub fn is_marked(&self, idx: u32) -> bool {
        let color = self.marks.get_color(idx);
        color == MarkColor::Gray || color == MarkColor::Black
    }

    pub fn marked_count(&self) -> usize {
        self.marked_count
    }

    pub fn gray_queue_size(&self) -> usize {
        self.gray_queue.len()
    }

    pub fn clear(&mut self) {
        self.marks.clear();
        self.gray_queue.clear();
        self.marked_count = 0;
    }

    pub fn process_gray_step(&mut self, heap: &HeapInner) -> Result<bool, GcError> {
        if let Some(idx) = self.gray_queue.pop_front() {
            self.mark_children(heap, idx)?;
            self.mark_black(idx);
            Ok(!self.gray_queue.is_empty())
        } else {
            Ok(false)
        }
    }

    pub fn mark_from_roots(&mut self, heap: &HeapInner, roots: &[u32]) -> Result<(), GcError> {
        for &root in roots {
            if is_old_idx(root) {
                let raw = old_idx_raw(root);
                if raw < heap.objects_len() {
                    self.mark_gray(raw);
                }
            }
        }

        while let Some(idx) = self.gray_queue.pop_front() {
            self.mark_children(heap, idx)?;
            self.mark_black(idx);
        }

        Ok(())
    }

    fn mark_children(&mut self, heap: &HeapInner, idx: u32) -> Result<(), GcError> {
        if let Some(Some(obj)) = heap.objects().get(idx as usize) {
            match obj {
                HeapObj::Str(_)
                | HeapObj::Symbol(_)
                | HeapObj::Char(_)
                | HeapObj::BigInt(_)
                | HeapObj::Decimal(_)
                | HeapObj::Range(_)
                | HeapObj::NativeFn(_, _)
                | HeapObj::FrozenModule(_)
                | HeapObj::VmValue(_) => {}
                HeapObj::Module(m) => {
                    let exports = m.exports.clone();
                    for val in exports {
                        if let Some(child_idx) = heap.get_heap_idx(val) {
                            self.mark_gray(child_idx);
                        }
                    }
                }
                HeapObj::Array(arr) => {
                    // Variant-aware: I64/F64 reprs hold raw numeric buffers
                    // with zero heap references, so there is nothing to mark
                    // — skip them entirely rather than boxing/copying just to
                    // scan. Only `Boxed` can hold child heap refs.
                    //
                    // Iterate the borrowed elements directly for `Boxed`.
                    // Cloning the whole Vec here made every mark O(n) *with
                    // an allocation*, so a large live array re-scanned each
                    // GC was O(n²) overall (and the throwaway clones
                    // generated more garbage). `mark_gray` only mutates the
                    // marker, never this cell, so holding the borrow across
                    // the loop is sound.
                    if let Some(items) = arr.as_boxed() {
                        for nv in items.iter() {
                            if let Some(child_idx) = heap.get_heap_idx(*nv) {
                                self.mark_gray(child_idx);
                            }
                        }
                    }
                }
                HeapObj::Object(obj_ref) => {
                    let guard = obj_ref.borrow();
                    let mut children = Vec::new();
                    guard.for_each_field(|_, nv| {
                        if let Some(child_idx) = heap.get_heap_idx(nv) {
                            children.push(child_idx);
                        }
                    });
                    for child_idx in children {
                        self.mark_gray(child_idx);
                    }
                    if let Some(cls) = guard.class() {
                        if let Some(ci) = heap.value_heap_idx(&varn_types::Value::Class(cls)) {
                            self.mark_gray(ci);
                        }
                    }
                }
                HeapObj::VmClosure(clos) => {
                    for upval in &clos.upvalues {
                        if let Ok(upval_inner) = upval.inner.try_borrow() {
                            if let Some(child_idx) = heap.get_heap_idx(upval_inner.value) {
                                self.mark_gray(child_idx);
                            }
                        }
                    }
                    for &constant in clos.constants.iter() {
                        if let Some(child_idx) = heap.get_heap_idx(constant) {
                            self.mark_gray(child_idx);
                        }
                    }
                }
                HeapObj::Class(cls) => {
                    let mark_value =
                        |marker: &mut TricolorMarker, heap: &HeapInner, v: &varn_types::Value| {
                            if let Some(ci) = heap.value_heap_idx(v) {
                                marker.mark_gray(ci);
                            }
                        };
                    for v in cls
                        .vtable
                        .borrow()
                        .iter()
                        .chain(cls.getter_vtable.borrow().iter())
                        .chain(cls.setter_vtable.borrow().iter())
                    {
                        mark_value(self, heap, v);
                    }
                    if let Some(super_cls) = cls.superclass.borrow().as_ref() {
                        mark_value(self, heap, &varn_types::Value::Class(super_cls.clone()));
                    }
                    for v in cls.statics.borrow().values() {
                        mark_value(self, heap, v);
                    }
                }
                HeapObj::Map(map_ref) => {
                    let m = map_ref.0.borrow();
                    for (k, v) in m.iter() {
                        if let Some(ci) = heap.get_heap_idx(k.0) {
                            self.mark_gray(ci);
                        }
                        if let Some(ci) = heap.get_heap_idx(*v) {
                            self.mark_gray(ci);
                        }
                    }
                }
                HeapObj::Set(set_ref) => {
                    let s = set_ref.0.borrow();
                    for k in s.iter() {
                        if let Some(ci) = heap.get_heap_idx(k.0) {
                            self.mark_gray(ci);
                        }
                    }
                }
                HeapObj::BoundMethod(bm) => {
                    if let Some(ci) = heap.value_heap_idx(&bm.receiver) {
                        self.mark_gray(ci);
                    }
                }
                HeapObj::Spread(spread_val) => {
                    if let Some(child_idx) = heap.get_heap_idx(*spread_val) {
                        self.mark_gray(child_idx);
                    }
                }
                HeapObj::EnumVariant(ev) => {
                    if let Some(ci) = heap.value_heap_idx(&ev.payload) {
                        self.mark_gray(ci);
                    }
                }
                HeapObj::Task(task) => {
                    let task = task.clone();
                    for v in &task.args {
                        if let Some(ci) = heap.value_heap_idx(v) {
                            self.mark_gray(ci);
                        }
                    }
                    for uv in &task.closure.upvalues {
                        if let Ok(inner) = uv.inner.try_borrow() {
                            if let Some(ci) = heap.value_heap_idx(&inner.value) {
                                self.mark_gray(ci);
                            }
                        }
                    }
                    for v in &task.closure.resolved_constants {
                        if let Some(ci) = heap.value_heap_idx(v) {
                            self.mark_gray(ci);
                        }
                    }
                }
                HeapObj::AsyncQueue(q) => {
                    let inner = q.0.borrow();
                    for v in &inner.queue {
                        if let Some(ci) = heap.value_heap_idx(v) {
                            self.mark_gray(ci);
                        }
                    }
                }
                HeapObj::TaskHandle(task) => match task.peek_state() {
                    varn_types::TaskState::Resolved(v) | varn_types::TaskState::Rejected(v) => {
                        if let Some(ci) = heap.value_heap_idx(&v) {
                            self.mark_gray(ci);
                        }
                    }
                    varn_types::TaskState::Pending => {}
                },
                HeapObj::Generator(gen) => {
                    let mut visit = |nv: varn_types::VmValue| {
                        if let Some(child_idx) = heap.get_heap_idx(nv) {
                            self.mark_gray(child_idx);
                        }
                    };
                    gen.0.trace_vm_values(&mut visit);

                    let mut visit_closure = |closure_ptr: usize| {
                        if let Some(&packed) = heap.identity_index().get(&closure_ptr) {
                            self.mark_gray(packed);
                        }
                    };
                    gen.0.trace_closures(&mut visit_closure);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct GcCollector {
    marker: TricolorMarker,
    swept_count: usize,
}

impl GcCollector {
    pub fn new(heap_capacity: u32) -> Self {
        Self {
            marker: TricolorMarker::new(heap_capacity),
            swept_count: 0,
        }
    }

    pub fn mark_phase(&mut self, heap: &HeapInner, roots: &[u32]) -> Result<(), GcError> {
        self.marker.clear();
        self.marker.mark_from_roots(heap, roots)?;
        Ok(())
    }

    pub fn sweep_phase(&mut self, heap: &mut HeapInner) -> Result<usize, GcError> {
        self.swept_count = 0;
        let mut freed = 0;

        for idx in 0..heap.objects_len() {
            let color = self.marker.get_color(idx);
            if color == MarkColor::White {
                if let Some(Some(_)) = heap.objects_mut().get_mut(idx as usize) {
                    heap.objects_mut()[idx as usize] = None;
                    heap.free_list_mut().push(idx);
                    freed += 1;
                    self.swept_count += 1;
                }
            }
        }

        Ok(freed)
    }

    pub fn collect(&mut self, heap: &mut HeapInner, roots: &[u32]) -> Result<usize, GcError> {
        self.mark_phase(heap, roots)?;
        let swept = self.sweep_phase(heap)?;
        Ok(swept)
    }

    pub fn incremental_mark_step(&mut self, heap: &HeapInner) -> Result<bool, GcError> {
        self.marker.process_gray_step(heap)
    }

    pub fn marked_count(&self) -> usize {
        self.marker.marked_count()
    }

    pub fn swept_count(&self) -> usize {
        self.swept_count
    }
}

#[cfg(test)]
mod array_scan_tests {
    use super::*;
    use crate::heap::HeapInner;
    use varn_types::{VmArray, VmValue};

    /// A.2: `mark_children`'s `HeapObj::Array` arm must branch on the repr
    /// (`as_boxed()`), never call the Boxed-only `borrow()` — that panics on
    /// a typed repr (see `VmArray::borrow`'s `unreachable_typed` cold path).
    /// I64/F64 hold no heap refs, so marking one must be a no-op: nothing
    /// queued gray, and — the actual regression this guards — no panic.
    #[test]
    fn mark_children_skips_typed_array_reprs_without_panicking() {
        let mut heap = HeapInner::new();
        let i64_idx = heap.alloc_raw(HeapObj::Array(VmArray::new_i64(vec![1, 2, 3])));
        let f64_idx = heap.alloc_raw(HeapObj::Array(VmArray::new_f64(vec![1.0, 2.0])));

        let mut marker = TricolorMarker::new(heap.objects_len());
        marker
            .mark_children(&heap, i64_idx)
            .expect("marking an I64 array must not error");
        marker
            .mark_children(&heap, f64_idx)
            .expect("marking an F64 array must not error");

        // Neither typed array holds a heap ref, so nothing should have been
        // queued gray by either scan.
        assert_eq!(marker.gray_queue_size(), 0);
        assert_eq!(marker.marked_count(), 0);
    }

    /// Control case: a `Boxed` array's heap-ref elements are still marked —
    /// the variant-aware rewrite must not regress the path every existing
    /// test in the suite exercises.
    #[test]
    fn mark_children_still_marks_boxed_array_heap_refs() {
        let mut heap = HeapInner::new();
        let inner_str_idx = heap.alloc_raw(HeapObj::Str(crate::heap::HeapStr::shared(
            std::rc::Rc::from("child"),
        )));
        let child = VmValue::from_heap_idx(inner_str_idx);
        let arr_idx = heap.alloc_raw(HeapObj::Array(VmArray::new(vec![
            child,
            VmValue::from_int(7),
        ])));

        let mut marker = TricolorMarker::new(heap.objects_len());
        marker
            .mark_children(&heap, arr_idx)
            .expect("marking a Boxed array must not error");

        assert_eq!(marker.gray_queue_size(), 1);
        assert_eq!(marker.get_color(inner_str_idx), MarkColor::Gray);
    }
}
