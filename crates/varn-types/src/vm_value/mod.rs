


















#[derive(Copy, Clone)]
#[repr(C)]
pub struct VmValue {
    tag: u64,
    payload: u64,
}












pub const KIND_MASK: u64 = 0xFF;

pub const KIND_NULL: u64 = 0;
pub const KIND_BOOL: u64 = 1;
pub const KIND_INT: u64 = 2;
pub const KIND_FLOAT: u64 = 3;


pub const KIND_HEAP: u64 = 4;


pub const KIND_SSO: u64 = 5;
pub const KIND_SYMBOL: u64 = 6;


const SSO_LEN_SHIFT: u32 = 8;

pub const SSO_MAX_LEN: usize = 5;

impl VmValue {
    
    
    
    
    #[inline(always)]
    pub const fn from_raw_parts(tag: u64, payload: u64) -> Self {
        Self { tag, payload }
    }

    
    
    
    #[inline(always)]
    pub const fn raw_tag(self) -> u64 {
        self.tag
    }

    
    #[inline(always)]
    pub const fn raw_payload(self) -> u64 {
        self.payload
    }

    
    #[inline(always)]
    pub const fn kind(self) -> u64 {
        self.tag & KIND_MASK
    }

    
    
    
    
    #[inline(always)]
    pub const fn ic_miss() -> Self {
        Self {
            tag: u64::MAX,
            payload: 0,
        }
    }

    
    #[inline(always)]
    pub const fn is_ic_miss(self) -> bool {
        self.tag == u64::MAX
    }

    
    
    
    #[inline(always)]
    pub fn bits_eq(self, other: Self) -> bool {
        self.tag == other.tag && self.payload == other.payload
    }

    #[inline(always)]
    pub const fn null() -> Self {
        Self {
            tag: KIND_NULL,
            payload: 0,
        }
    }

    #[inline(always)]
    pub const fn bool_true() -> Self {
        Self {
            tag: KIND_BOOL,
            payload: 1,
        }
    }

    #[inline(always)]
    pub const fn bool_false() -> Self {
        Self {
            tag: KIND_BOOL,
            payload: 0,
        }
    }

    #[inline(always)]
    pub fn from_bool(b: bool) -> Self {
        Self {
            tag: KIND_BOOL,
            payload: b as u64,
        }
    }

    
    
    
    #[inline(always)]
    pub fn from_int(n: i64) -> Self {
        Self {
            tag: KIND_INT,
            payload: n as u64,
        }
    }

    
    
    
    #[inline(always)]
    pub fn from_int_wrapping(n: i64) -> Self {
        Self::from_int(n)
    }

    #[inline(always)]
    pub fn from_i32(n: i32) -> Self {
        Self::from_int(n as i64)
    }

    
    
    #[inline(always)]
    pub fn from_f64(n: f64) -> Self {
        Self {
            tag: KIND_FLOAT,
            payload: n.to_bits(),
        }
    }

    #[inline(always)]
    pub fn from_heap(r: HeapRef) -> Self {
        Self {
            tag: KIND_HEAP,
            payload: r.addr(),
        }
    }

    #[inline(always)]
    pub fn is_f64(self) -> bool {
        self.kind() == KIND_FLOAT
    }

    #[inline(always)]
    pub fn is_null(self) -> bool {
        self.kind() == KIND_NULL
    }

    #[inline(always)]
    pub fn is_bool(self) -> bool {
        self.kind() == KIND_BOOL
    }

    #[inline(always)]
    pub fn is_int(self) -> bool {
        self.kind() == KIND_INT
    }

    #[inline(always)]
    pub fn is_heap(self) -> bool {
        self.kind() == KIND_HEAP
    }

    #[inline(always)]
    pub fn is_sso(self) -> bool {
        self.kind() == KIND_SSO
    }

    #[inline(always)]
    pub fn try_from_sso(s: &str) -> Option<Self> {
        let b = s.as_bytes();
        if b.len() > SSO_MAX_LEN {
            return None;
        }
        let mut packed: u64 = 0;
        for (i, &byte) in b.iter().enumerate() {
            packed |= (byte as u64) << (i as u32 * 8);
        }
        Some(Self {
            tag: KIND_SSO | ((b.len() as u64) << SSO_LEN_SHIFT),
            payload: packed,
        })
    }

    #[inline(always)]
    pub fn from_sso_raw(len: usize, packed: u64) -> Self {
        Self {
            tag: KIND_SSO | ((len as u64) << SSO_LEN_SHIFT),
            payload: packed,
        }
    }

    #[inline(always)]
    pub fn sso_len(self) -> usize {
        ((self.tag >> SSO_LEN_SHIFT) & 0xFF) as usize
    }

    #[inline(always)]
    pub fn sso_copy_bytes(self, buf: &mut [u8; SSO_MAX_LEN]) -> usize {
        let len = self.sso_len();
        for (i, slot) in buf.iter_mut().enumerate().take(len) {
            *slot = ((self.payload >> (i as u32 * 8)) & 0xFF) as u8;
        }
        len
    }

    #[inline(always)]
    pub fn sso_eq_bytes(self, bytes: &[u8]) -> bool {
        let len = self.sso_len();
        if bytes.len() != len {
            return false;
        }
        let mut buf = [0u8; SSO_MAX_LEN];
        self.sso_copy_bytes(&mut buf);
        &buf[..len] == bytes
    }

    #[inline(always)]
    pub fn sso_as_str(self, buf: &mut [u8; SSO_MAX_LEN]) -> &str {
        let len = self.sso_copy_bytes(buf);

        unsafe { std::str::from_utf8_unchecked(&buf[..len]) }
    }

    #[inline(always)]
    pub fn as_f64(self) -> f64 {
        f64::from_bits(self.payload)
    }

    #[inline(always)]
    pub fn as_int(self) -> i64 {
        self.payload as i64
    }

    #[inline(always)]
    pub fn as_i32(self) -> i32 {
        self.as_int() as i32
    }

    #[inline(always)]
    pub fn as_bool(self) -> bool {
        self.payload != 0
    }

    
    #[inline(always)]
    pub fn as_heap(self) -> HeapRef {
        debug_assert!(self.is_heap() && self.payload != 0);
        unsafe { HeapRef::from_addr_unchecked(self.payload) }
    }

    #[inline(always)]
    pub fn is_truthy(self) -> bool {
        if self.is_null() {
            return false;
        }
        if self.is_bool() {
            return self.as_bool();
        }
        if self.is_int() {
            return self.as_int() != 0;
        }
        if self.is_f64() {
            let f = self.as_f64();
            return f != 0.0 && !f.is_nan();
        }
        true
    }

    #[inline(always)]
    pub fn to_f64(self) -> f64 {
        if self.is_f64() {
            self.as_f64()
        } else if self.is_int() {
            self.as_int() as f64
        } else if self.is_bool() {
            if self.as_bool() {
                1.0
            } else {
                0.0
            }
        } else {
            f64::NAN
        }
    }

    #[inline(always)]
    pub fn to_i32(self) -> i32 {
        if self.is_int() {
            self.as_i32()
        } else if self.is_f64() {
            self.as_f64() as i32
        } else if self.is_bool() {
            if self.as_bool() {
                1
            } else {
                0
            }
        } else {
            0
        }
    }
}

mod array;
mod heap_ref;
mod traits;
mod vm_array;
mod vm_array_access;

pub use array::{ArrayRepr, BoxedElems};
pub use heap_ref::HeapRef;
pub use vm_array::VmArray;
