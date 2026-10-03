/// A VM value: an explicit tag word plus a full 64-bit payload.
///
/// Not a NaN-box: Varn knows its types before it emits an opcode, so it has
/// no need for a dynamic engine's one-word encoding and its 48-bit payload
/// limit. `int` is a native `i64`, `float` a real `f64`, and a heap
/// reference gets a whole word (room for a direct pointer later). Two
/// 64-bit registers cost the same as one on x86-64 and aarch64; the masking
/// a NaN-box needs did not.
///
/// `#[repr(C)]` with two `u64`s — not `u128` — keeps the alignment at 8, so
/// the DST tail of [`crate::value::ObjData`] still starts on a word boundary
/// and the JIT can address a stack slot as two adjacent words.
///
/// Both fields are PRIVATE. Every producer and consumer goes through the
/// constructors and accessors below, so a further change of representation is
/// a change to this file. The escape hatches
/// [`VmValue::from_raw_parts`]/[`VmValue::raw_tag`]/[`VmValue::raw_payload`]
/// exist for the JIT, which re-emits the encoding inline; they move bits and
/// never interpret them.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct VmValue {
    tag: u64,
    payload: u64,
}

// ── Tag word ────────────────────────────────────────────────────────────
//
// The low byte is the kind. The bytes above it are per-kind metadata, used
// today only by SSO for its length. A whole word for a 3-bit answer is
// deliberate: comparing a tag is now one `cmp` against a small immediate,
// where the NaN-box needed a mask, a shift and a 64-bit constant that would
// not fit in an instruction and had to be loaded from a constant pool.
//
// Values are dense from 0 so a `match` on the kind lowers to a jump table.

/// Kind of a value: the low byte of the tag word.
pub const KIND_MASK: u64 = 0xFF;

pub const KIND_NULL: u64 = 0;
pub const KIND_BOOL: u64 = 1;
pub const KIND_INT: u64 = 2;
pub const KIND_FLOAT: u64 = 3;
/// Heap reference. Payload is the heap-table index today, and has room for a
/// direct pointer when that table goes away.
pub const KIND_HEAP: u64 = 4;
/// Small string stored inline in the payload; length in the tag above the
/// kind byte.
pub const KIND_SSO: u64 = 5;
pub const KIND_SYMBOL: u64 = 6;

/// Bit position, inside the tag word, of the SSO length.
const SSO_LEN_SHIFT: u32 = 8;

pub const SSO_MAX_LEN: usize = 5;

impl VmValue {
    /// Assemble a value from its two words. Only for code that received them
    /// *from* [`Self::raw_tag`]/[`Self::raw_payload`] — JIT-produced values,
    /// safepoint spills, layout probes. Never to synthesize an encoding by
    /// hand: use a constructor.
    #[inline(always)]
    pub const fn from_raw_parts(tag: u64, payload: u64) -> Self {
        Self { tag, payload }
    }

    /// The tag word, for code that round-trips it through
    /// [`Self::from_raw_parts`]. Not for inspection — every question about a
    /// value has an accessor.
    #[inline(always)]
    pub const fn raw_tag(self) -> u64 {
        self.tag
    }

    /// The payload word. Same contract as [`Self::raw_tag`].
    #[inline(always)]
    pub const fn raw_payload(self) -> u64 {
        self.payload
    }

    /// This value's kind, as one of the `KIND_*` constants.
    #[inline(always)]
    pub const fn kind(self) -> u64 {
        self.tag & KIND_MASK
    }

    /// The "no answer" sentinel the inline-cache fast paths return on a miss.
    ///
    /// A tag no constructor produces, so it can never collide with a real
    /// result the helper could have found.
    #[inline(always)]
    pub const fn ic_miss() -> Self {
        Self {
            tag: u64::MAX,
            payload: 0,
        }
    }

    /// Whether this is the [`Self::ic_miss`] sentinel.
    #[inline(always)]
    pub const fn is_ic_miss(self) -> bool {
        self.tag == u64::MAX
    }

    /// Whether two values have the *same representation*. Narrower than `==`,
    /// which coerces int and float; this is what SSO string comparison and
    /// identity checks want.
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

    /// Carry an `i64`. Every `i64` is a valid `int` and the payload is a full
    /// word, so this is exact for the whole range — no mask, no truncation,
    /// no range preconditions on the caller.
    #[inline(always)]
    pub fn from_int(n: i64) -> Self {
        Self {
            tag: KIND_INT,
            payload: n as u64,
        }
    }

    /// Kept as the name shift operations use to say their result width is
    /// part of the operation. Nothing wraps any more: the payload holds
    /// every `i64`.
    #[inline(always)]
    pub fn from_int_wrapping(n: i64) -> Self {
        Self::from_int(n)
    }

    #[inline(always)]
    pub fn from_i32(n: i32) -> Self {
        Self::from_int(n as i64)
    }

    /// Carry an `f64`, NaN included: the tag is its own word, so the payload
    /// is just the IEEE bits (spec §4).
    #[inline(always)]
    pub fn from_f64(n: f64) -> Self {
        Self {
            tag: KIND_FLOAT,
            payload: n.to_bits(),
        }
    }

    #[inline(always)]
    pub fn from_heap_idx(idx: u32) -> Self {
        Self {
            tag: KIND_HEAP,
            payload: idx as u64,
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
    pub fn as_heap_idx(self) -> u32 {
        self.payload as u32
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
mod traits;
mod vm_array;
mod vm_array_access;

pub use array::{ArrayRepr, BoxedElems};
pub use vm_array::VmArray;
