use std::cell::RefCell;
use std::rc::Rc;

/// A high-performance, zero-copy byte buffer with sub-slice support.
#[derive(Clone, Debug)]
pub struct VmBuffer {
    data: Rc<RefCell<Vec<u8>>>,
    offset: u32,
    length: u32,
}

impl VmBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            data: Rc::new(RefCell::new(vec![0u8; capacity])),
            offset: 0,
            length: capacity as u32,
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            data: Rc::new(RefCell::new(bytes.to_vec())),
            offset: 0,
            length: bytes.len() as u32,
        }
    }

    pub fn slice(&self, start: usize, end: usize) -> Self {
        let start = (self.offset as usize + start).min(self.offset as usize + self.length as usize);
        let end = (self.offset as usize + end).min(self.offset as usize + self.length as usize);
        let len = if end >= start { end - start } else { 0 };
        Self {
            data: Rc::clone(&self.data),
            offset: start as u32,
            length: len as u32,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.length as usize
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    #[inline]
    pub fn as_slice(&self) -> std::cell::Ref<'_, [u8]> {
        std::cell::Ref::map(self.data.borrow(), |b| {
            let start = self.offset as usize;
            let end = (self.offset + self.length) as usize;
            if start < b.len() && end <= b.len() {
                &b[start..end]
            } else {
                &[]
            }
        })
    }

    #[inline]
    pub fn as_mut_slice(&self) -> std::cell::RefMut<'_, [u8]> {
        std::cell::RefMut::map(self.data.borrow_mut(), |b| {
            let start = self.offset as usize;
            let end = (self.offset + self.length) as usize;
            if start < b.len() && end <= b.len() {
                &mut b[start..end]
            } else {
                &mut []
            }
        })
    }
}
