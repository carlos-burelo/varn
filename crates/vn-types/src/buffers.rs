use crate::VmValue;

pub trait Buffers {
    fn alloc_buffer(&mut self, size: usize) -> VmValue;

    fn alloc_buffer_from_bytes(&mut self, bytes: &[u8]) -> VmValue;

    fn is_buffer(&self, _v: VmValue) -> bool {
        false
    }

    fn buffer_len(&self, _v: VmValue) -> usize {
        0
    }

    fn buffer_get_byte(&self, _v: VmValue, _idx: usize) -> Option<u8> {
        None
    }

    fn buffer_set_byte(&mut self, _v: VmValue, _idx: usize, _byte: u8) -> bool {
        false
    }

    fn buffer_slice(&mut self, _v: VmValue, _start: usize, _end: usize) -> Option<VmValue> {
        None
    }

    fn buffer_to_string(&self, _v: VmValue) -> Option<String> {
        None
    }

    fn buffer_to_bytes(&self, _v: VmValue) -> Option<Vec<u8>> {
        None
    }
}
