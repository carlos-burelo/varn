use super::VmValue;

#[repr(C, u8)]
#[derive(Debug)]
pub enum ArrayRepr {
    Boxed(BoxedElems) = 0,

    I64(Vec<i64>) = 1,

    F64(Vec<f64>) = 2,
}

#[repr(C)]
#[derive(Debug)]
pub struct BoxedElems {
    pub(super) items: Vec<VmValue>,
}

impl BoxedElems {
    #[inline(always)]
    pub fn new(items: Vec<VmValue>) -> Self {
        Self { items }
    }

    #[inline(always)]
    pub fn as_vec(&self) -> &Vec<VmValue> {
        &self.items
    }
}

impl std::ops::Deref for BoxedElems {
    type Target = Vec<VmValue>;

    #[inline(always)]
    fn deref(&self) -> &Vec<VmValue> {
        &self.items
    }
}

impl ArrayRepr {
    pub const DISC_OFF: usize = 0;

    pub const ELEMS_UNION_OFF: usize = 8;

    #[inline(always)]
    pub fn discriminant(&self) -> u8 {
        match self {
            ArrayRepr::Boxed(_) => 0,
            ArrayRepr::I64(_) => 1,
            ArrayRepr::F64(_) => 2,
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        match self {
            ArrayRepr::Boxed(v) => v.len(),
            ArrayRepr::I64(v) => v.len(),
            ArrayRepr::F64(v) => v.len(),
        }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn heap_bytes(&self) -> u64 {
        match self {
            ArrayRepr::Boxed(v) => (v.items.capacity() * std::mem::size_of::<VmValue>()) as u64,
            ArrayRepr::I64(v) => (v.capacity() * std::mem::size_of::<i64>()) as u64,
            ArrayRepr::F64(v) => (v.capacity() * std::mem::size_of::<f64>()) as u64,
        }
    }
}

const _: () = {
    assert!(std::mem::align_of::<ArrayRepr>() == 8);
    assert!(ArrayRepr::ELEMS_UNION_OFF == 8);
    assert!(std::mem::size_of::<ArrayRepr>().is_multiple_of(8));
};
