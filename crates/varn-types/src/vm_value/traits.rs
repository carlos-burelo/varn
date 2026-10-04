use std::fmt;

use super::{VmValue, SSO_MAX_LEN};

impl PartialEq for VmValue {
    fn eq(&self, other: &Self) -> bool {
        if self.bits_eq(*other) {
            return true;
        }
        if (self.is_int() || self.is_f64()) && (other.is_int() || other.is_f64()) {
            return self.to_f64() == other.to_f64();
        }
        false
    }
}

impl Eq for VmValue {}

/// Hashes the value's representation, not its numeric meaning.
///
/// This is deliberately *narrower* than [`PartialEq`] above, which coerces
/// int/float. Every keyed container in the VM canonicalizes through
/// `Heap::canonical_map_key` before hashing, so two keys that compare equal
/// arrive here with identical bits.
impl std::hash::Hash for VmValue {
    #[inline(always)]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.tag.hash(state);
        self.payload.hash(state);
    }
}

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
            let mut buf = [0u8; SSO_MAX_LEN];
            let s = self.sso_as_str(&mut buf);
            write!(f, "sso({:?})", s)
        } else if self.is_heap() {
            write!(f, "heap[{:#x}]", self.as_heap().addr())
        } else if self.is_ic_miss() {
            write!(f, "<ic-miss>")
        } else {
            write!(
                f,
                "unknown(tag=0x{:016x}, payload=0x{:016x})",
                self.tag, self.payload
            )
        }
    }
}

impl fmt::Display for VmValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}
