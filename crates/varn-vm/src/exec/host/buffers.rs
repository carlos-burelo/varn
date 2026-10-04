use super::*;

impl ExecCtx {
    pub(super) fn host_alloc_buffer(&mut self, size: usize) -> VmValue {
        self.heap.alloc_vm_buffer(varn_types::VmBuffer::new(size))
    }
    pub(super) fn host_alloc_buffer_from_bytes(&mut self, bytes: &[u8]) -> VmValue {
        self.heap
            .alloc_vm_buffer(varn_types::VmBuffer::from_bytes(bytes))
    }
    pub(super) fn host_is_buffer(&self, v: VmValue) -> bool {
        if v.is_heap() {
            matches!(self.heap.get(v.as_heap()), Some(HeapObj::Buffer(_)))
        } else {
            false
        }
    }
    pub(super) fn host_buffer_len(&self, v: VmValue) -> usize {
        if v.is_heap() {
            if let Some(HeapObj::Buffer(b)) = self.heap.get(v.as_heap()) {
                return b.len();
            }
        }
        0
    }
    pub(super) fn host_buffer_get_byte(&self, v: VmValue, idx: usize) -> Option<u8> {
        if v.is_heap() {
            if let Some(HeapObj::Buffer(b)) = self.heap.get(v.as_heap()) {
                return b.as_slice().get(idx).copied();
            }
        }
        None
    }
    pub(super) fn host_buffer_set_byte(&mut self, v: VmValue, idx: usize, byte: u8) -> bool {
        if v.is_heap() {
            if let Some(HeapObj::Buffer(b)) = self.heap.get_mut(v.as_heap()) {
                let mut slice = b.as_mut_slice();
                if idx < slice.len() {
                    slice[idx] = byte;
                    return true;
                }
            }
        }
        false
    }
    pub(super) fn host_buffer_slice(
        &mut self,
        v: VmValue,
        start: usize,
        end: usize,
    ) -> Option<VmValue> {
        if v.is_heap() {
            if let Some(HeapObj::Buffer(b)) = self.heap.get(v.as_heap()) {
                let sub = b.slice(start, end);
                return Some(self.heap.alloc_vm_buffer(sub));
            }
        }
        None
    }
    pub(super) fn host_buffer_to_string(&self, v: VmValue) -> Option<String> {
        if v.is_heap() {
            if let Some(HeapObj::Buffer(b)) = self.heap.get(v.as_heap()) {
                let slice = b.as_slice();
                return String::from_utf8(slice.to_vec()).ok();
            }
        }
        None
    }
    pub(super) fn host_buffer_to_bytes(&self, v: VmValue) -> Option<Vec<u8>> {
        if v.is_heap() {
            if let Some(HeapObj::Buffer(b)) = self.heap.get(v.as_heap()) {
                return Some(b.as_slice().to_vec());
            }
        }
        None
    }
}
