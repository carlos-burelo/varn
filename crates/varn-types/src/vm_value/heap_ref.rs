



use std::num::NonZeroU64;

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct HeapRef(NonZeroU64);

impl HeapRef {
    #[inline(always)]
    pub fn from_addr(addr: u64) -> Option<Self> {
        NonZeroU64::new(addr).map(Self)
    }

    
    
    #[inline(always)]
    pub unsafe fn from_addr_unchecked(addr: u64) -> Self {
        Self(NonZeroU64::new_unchecked(addr))
    }

    #[inline(always)]
    pub fn addr(self) -> u64 {
        self.0.get()
    }

    #[inline(always)]
    pub fn as_ptr<T>(self) -> *mut T {
        self.0.get() as usize as *mut T
    }
}
