use std::fmt;
use varn_base::VmValuePayload;

#[derive(Copy, Clone)]
#[repr(transparent)]
pub struct VmValue(pub u64);

#[derive(Debug, Clone, Copy)]
pub struct VmValueRef(pub VmValue);

impl VmValuePayload for VmValueRef {
    fn clone_payload(&self) -> Box<dyn VmValuePayload> {
        Box::new(*self)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub const QNAN: u64 = 0x7FF8_0000_0000_0000;
pub const SIGN: u64 = 0x8000_0000_0000_0000;

const TAG_NULL: u64 = 0x0001_0000_0000_0000;
const TAG_FALSE: u64 = 0x0002_0000_0000_0000;
const TAG_TRUE: u64 = 0x0003_0000_0000_0000;
const TAG_INT: u64 = 0x0004_0000_0000_0000;
const TAG_PTR: u64 = 0x0005_0000_0000_0000;

pub const TAG_SSO: u64 = 0x0006_0000_0000_0000;
pub const TAG_SYMBOL: u64 = 0x0007_0000_0000_0000;

const MASK_TAG: u64 = 0x0007_0000_0000_0000;
const MASK_LOW32: u64 = 0x0000_0000_FFFF_FFFF;
const MASK_INT48: u64 = 0x0000_FFFF_FFFF_FFFF;
const SIGN_BIT_47: u64 = 0x0000_8000_0000_0000;

impl VmValue {
    #[inline(always)]
    pub const fn null() -> Self {
        Self(QNAN | TAG_NULL)
    }

    #[inline(always)]
    pub const fn bool_true() -> Self {
        Self(QNAN | TAG_TRUE)
    }

    #[inline(always)]
    pub const fn bool_false() -> Self {
        Self(QNAN | TAG_FALSE)
    }

    #[inline(always)]
    pub fn from_bool(b: bool) -> Self {
        if b {
            Self::bool_true()
        } else {
            Self::bool_false()
        }
    }

    #[inline(always)]
    pub fn from_int(n: i64) -> Self {
        Self(QNAN | TAG_INT | ((n as u64) & MASK_INT48))
    }

    #[inline(always)]
    pub fn from_i32(n: i32) -> Self {
        Self::from_int(n as i64)
    }

    #[inline(always)]
    pub fn from_f64(n: f64) -> Self {
        let bits = n.to_bits();
        if (bits & QNAN) == QNAN {
            return Self::null();
        }
        Self(bits)
    }

    #[inline(always)]
    pub fn from_heap_idx(idx: u32) -> Self {
        Self(SIGN | QNAN | TAG_PTR | idx as u64)
    }

    #[inline(always)]
    pub fn from_symbol_idx(idx: u32) -> Self {
        Self(QNAN | TAG_SYMBOL | idx as u64)
    }

    #[inline(always)]
    pub fn is_f64(self) -> bool {
        (self.0 & QNAN) != QNAN
    }

    #[inline(always)]
    pub fn is_null(self) -> bool {
        (self.0 & (QNAN | MASK_TAG)) == (QNAN | TAG_NULL)
    }

    #[inline(always)]
    pub fn is_bool(self) -> bool {
        let tag = self.0 & (QNAN | MASK_TAG);
        tag == (QNAN | TAG_FALSE) || tag == (QNAN | TAG_TRUE)
    }

    #[inline(always)]
    pub fn is_int(self) -> bool {
        (self.0 & (QNAN | MASK_TAG)) == (QNAN | TAG_INT)
    }

    #[inline(always)]
    pub fn is_heap(self) -> bool {
        (self.0 & (SIGN | QNAN | MASK_TAG)) == (SIGN | QNAN | TAG_PTR)
    }

    #[inline(always)]
    pub fn is_symbol(self) -> bool {
        (self.0 & (QNAN | MASK_TAG)) == (QNAN | TAG_SYMBOL)
    }

    #[inline(always)]
    pub fn is_sso(self) -> bool {
        (self.0 & (SIGN | QNAN | MASK_TAG)) == (QNAN | TAG_SSO)
    }

    #[inline(always)]
    pub fn try_from_sso(s: &str) -> Option<Self> {
        let b = s.as_bytes();
        if b.len() > 5 {
            return None;
        }
        if b.iter().any(|&c| c > 127) {
            return None;
        }
        let mut v: u64 = (b.len() as u64) << 45;
        for (i, &byte) in b.iter().enumerate() {
            v |= (byte as u64) << (37 - i as u32 * 8);
        }
        Some(VmValue(QNAN | TAG_SSO | v))
    }

    #[inline(always)]
    pub fn sso_len(self) -> usize {
        ((self.0 >> 45) & 0x7) as usize
    }

    #[inline(always)]
    pub fn sso_copy_bytes(self, buf: &mut [u8; 5]) -> usize {
        let len = self.sso_len();
        for i in 0..len {
            buf[i] = ((self.0 >> (37 - i as u32 * 8)) & 0xFF) as u8;
        }
        len
    }

    #[inline]
    pub fn sso_as_str<'a>(self, buf: &'a mut [u8; 5]) -> &'a str {
        let len = self.sso_copy_bytes(buf);

        unsafe { std::str::from_utf8_unchecked(&buf[..len]) }
    }

    #[inline(always)]
    pub fn as_f64(self) -> f64 {
        f64::from_bits(self.0)
    }

    #[inline(always)]
    pub fn as_int(self) -> i64 {
        let raw = self.0 & MASK_INT48;
        if raw & SIGN_BIT_47 != 0 {
            (raw | !MASK_INT48) as i64
        } else {
            raw as i64
        }
    }

    #[inline(always)]
    pub fn as_i32(self) -> i32 {
        self.as_int() as i32
    }

    #[inline(always)]
    pub fn as_bool(self) -> bool {
        (self.0 & MASK_TAG) == TAG_TRUE
    }

    #[inline(always)]
    pub fn as_heap_idx(self) -> u32 {
        (self.0 & MASK_LOW32) as u32
    }

    #[inline(always)]
    pub fn as_symbol_idx(self) -> u32 {
        (self.0 & MASK_LOW32) as u32
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

impl PartialEq for VmValue {
    fn eq(&self, other: &Self) -> bool {
        if self.0 == other.0 {
            return true;
        }
        if (self.is_int() || self.is_f64()) && (other.is_int() || other.is_f64()) {
            return self.to_f64() == other.to_f64();
        }
        false
    }
}

impl Eq for VmValue {}

impl fmt::Debug for VmValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_f64() {
            write!(f, "f64({})", self.as_f64())
        } else if self.is_int() {
            write!(f, "i64({})", self.as_int())
        } else if self.is_bool() {
            write!(f, "bool({})", self.as_bool())
        } else if self.is_null() {
            write!(f, "null")
        } else if self.is_sso() {
            let mut buf = [0u8; 5];
            let s = self.sso_as_str(&mut buf);
            write!(f, "sso({:?})", s)
        } else if self.is_heap() {
            write!(f, "heap[{}]", self.as_heap_idx())
        } else {
            write!(f, "nan(0x{:016x})", self.0)
        }
    }
}

impl fmt::Display for VmValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct VmArray(pub Rc<RefCell<Vec<VmValue>>>);

impl VmArray {
    #[inline]
    pub fn new(items: Vec<VmValue>) -> Self {
        Self(Rc::new(RefCell::new(items)))
    }

    #[inline]
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    #[inline]
    pub fn borrow(&self) -> std::cell::Ref<'_, Vec<VmValue>> {
        self.0.borrow()
    }

    #[inline]
    pub fn borrow_mut(&self) -> std::cell::RefMut<'_, Vec<VmValue>> {
        self.0.borrow_mut()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }
}

impl PartialEq for VmArray {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for VmArray {}

impl std::hash::Hash for VmArray {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}
