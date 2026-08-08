use std::rc::Rc;
use varn_types::RuntimeString;

/// Lazily-computed ASCII cache for a `HeapStr`. The viewed content of a
/// `HeapStr` is immutable (a `Shared` string is frozen; an `Ext` prefix view
/// never changes once created), so once computed the answer is stable for the
/// lifetime of the instance.
pub mod ascii_flag {
    pub const UNKNOWN: u8 = 0;
    pub const YES: u8 = 1;
    pub const NO: u8 = 2;
}

/// Byte length limit for `HeapStr::Slice` (30 bits); the two high bits of
/// `len_ascii` hold the ASCII flag so the variant stays within the enum's
/// existing payload size.
const SLICE_LEN_MASK: u32 = 0x3FFF_FFFF;

/// Bytes a short dynamic string keeps inside its heap object instead of behind
/// an `Rc`.
///
/// Measured, not chosen: 37 is the largest value that leaves
/// `size_of::<HeapObj>()` at 48. 38 takes it to 64, and that number is the slot
/// stride every heap type shares — a third more memory per object of every kind
/// to help strings would be a bad trade nothing in a string benchmark would
/// show. Pinned by `heap_obj_slot_stride_is_unchanged`.
pub const INLINE_STR_CAP: usize = 37;

/// Heap string payload. `Shared` is an immutable interned/frozen string;
/// `Ext` is a prefix view (`buf[..len]`) of a shared growable buffer, the
/// representation `str_concat` produces for accumulation patterns. Appending
/// to the buffer's tip never changes an existing prefix, so older views stay
/// valid without any aliasing analysis; a view that is no longer the tip is
/// copied out before growing. Single-threaded interior mutability via
/// `UnsafeCell` mirrors `ArrayRef`. `Slice` is a zero-copy substring view of
/// an immutable `Shared` buffer (`src[off..off+len]`), the representation
/// `substring`/`slice` produce; note it retains the whole source buffer for
/// its lifetime (the JS-engine substring-view trade-off).
#[derive(Clone)]
pub enum HeapStr {
    Shared(RuntimeString, std::cell::Cell<u8>),
    Ext {
        buf: Rc<std::cell::UnsafeCell<String>>,
        len: usize,
        ascii: std::cell::Cell<u8>,
    },
    Slice {
        src: RuntimeString,
        off: u32,
        /// bits 0..30 = byte length, bits 30..32 = `ascii_flag` state.
        len_ascii: std::cell::Cell<u32>,
    },
    /// A short dynamic string stored IN the heap object, with no `Rc` behind
    /// it. `alloc_str_dynamic`'s `Rc::from` is a malloc plus a copy for every
    /// string over the 5-byte SSO limit, and the common `"prefix" + <small
    /// int>` result lands just past it. Capacity is chosen so
    /// `size_of::<HeapObj>()` does not change — the slot stride is shared with
    /// every other heap type, so widening it to help strings would tax every
    /// other allocation.
    ///
    /// Not sliceable in place: a `Slice` view borrows an `Rc` buffer that
    /// outlives any collection, whereas these bytes live in the heap slot and
    /// move with it. `alloc_substring` copies out of one instead.
    Inline {
        len: u8,
        ascii: std::cell::Cell<u8>,
        bytes: [u8; INLINE_STR_CAP],
    },
}

impl HeapStr {
    #[inline]
    pub(crate) fn shared(s: RuntimeString) -> Self {
        HeapStr::Shared(s, std::cell::Cell::new(ascii_flag::UNKNOWN))
    }

    #[inline]
    pub(crate) fn ext(buf: Rc<std::cell::UnsafeCell<String>>, len: usize, ascii: u8) -> Self {
        HeapStr::Ext {
            buf,
            len,
            ascii: std::cell::Cell::new(ascii),
        }
    }

    /// Store `s` inside the heap object. Caller guarantees
    /// `s.len() <= INLINE_STR_CAP`.
    #[inline]
    pub(crate) fn inline(s: &str) -> Self {
        debug_assert!(s.len() <= INLINE_STR_CAP);
        let mut bytes = [0u8; INLINE_STR_CAP];
        bytes[..s.len()].copy_from_slice(s.as_bytes());
        HeapStr::Inline {
            len: s.len() as u8,
            ascii: std::cell::Cell::new(ascii_flag::UNKNOWN),
            bytes,
        }
    }

    /// Zero-copy substring view over an immutable shared buffer. Caller
    /// guarantees `off..off+len` lies on char boundaries and `len` fits
    /// [`SLICE_LEN_MASK`].
    #[inline]
    pub(crate) fn slice_of(src: RuntimeString, off: usize, len: usize, ascii: u8) -> Self {
        debug_assert!(len as u32 <= SLICE_LEN_MASK);
        HeapStr::Slice {
            src,
            off: off as u32,
            len_ascii: std::cell::Cell::new(len as u32 | ((ascii as u32) << 30)),
        }
    }

    #[inline]
    pub(crate) fn as_str(&self) -> &str {
        match self {
            HeapStr::Shared(s, _) => s,
            // Safety: single-threaded VM; the buffer is only appended to (via
            // `str_concat`), never while a borrow from this view is live.
            HeapStr::Ext { buf, len, .. } => unsafe { &(&*buf.get())[..*len] },
            HeapStr::Slice { src, off, len_ascii } => {
                let off = *off as usize;
                let len = (len_ascii.get() & SLICE_LEN_MASK) as usize;
                &src[off..off + len]
            }
            // Safety: `Inline` is only ever built from a `&str` in
            // `alloc_str_dynamic`, which copies whole bytes, so the prefix is
            // valid UTF-8 by construction.
            HeapStr::Inline { len, bytes, .. } => unsafe {
                std::str::from_utf8_unchecked(&bytes[..*len as usize])
            },
        }
    }

    #[inline]
    pub(crate) fn ascii_state(&self) -> u8 {
        match self {
            HeapStr::Shared(_, ascii) => ascii.get(),
            HeapStr::Ext { ascii, .. } => ascii.get(),
            HeapStr::Slice { len_ascii, .. } => (len_ascii.get() >> 30) as u8,
            HeapStr::Inline { ascii, .. } => ascii.get(),
        }
    }

    #[inline]
    pub(crate) fn is_ascii(&self) -> bool {
        match self.ascii_state() {
            ascii_flag::YES => true,
            ascii_flag::NO => false,
            _ => {
                let is = self.as_str().is_ascii();
                self.set_ascii_state(if is {
                    ascii_flag::YES
                } else {
                    ascii_flag::NO
                });
                is
            }
        }
    }

    #[inline]
    pub(crate) fn is_ascii_cached(&self) -> bool {
        self.ascii_state() == ascii_flag::YES
    }

    #[inline]
    pub(crate) fn to_shared(&self) -> RuntimeString {
        match self {
            HeapStr::Shared(s, _) => Rc::clone(s),
            HeapStr::Ext { buf, len, .. } => {
                let slice = unsafe { &(&*buf.get())[..*len] };
                Rc::from(slice)
            }
            HeapStr::Slice { src, off, len_ascii } => {
                let off = *off as usize;
                let len = (len_ascii.get() & SLICE_LEN_MASK) as usize;
                Rc::from(&src[off..off + len])
            }
            HeapStr::Inline { len, bytes, .. } => {
                let slice = unsafe { std::str::from_utf8_unchecked(&bytes[..*len as usize]) };
                Rc::from(slice)
            }
        }
    }

    #[inline]
    fn set_ascii_state(&self, state: u8) {
        match self {
            HeapStr::Shared(_, ascii) => ascii.set(state),
            HeapStr::Ext { ascii, .. } => ascii.set(state),
            HeapStr::Slice { len_ascii, .. } => {
                let len = len_ascii.get() & SLICE_LEN_MASK;
                len_ascii.set(len | ((state as u32) << 30));
            }
            HeapStr::Inline { ascii, .. } => ascii.set(state),
        }
    }

    /// True when this view ends exactly at the buffer tip, i.e. appending to
    /// the buffer extends this string without disturbing any other view.
    #[inline]
    pub(crate) fn is_tip(&self) -> bool {
        match self {
            HeapStr::Ext { buf, len, .. } => unsafe { (&*buf.get()).len() == *len },
            _ => false,
        }
    }
}

impl std::fmt::Debug for HeapStr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("HeapStr").field(&self.as_str()).finish()
    }
}

impl std::fmt::Display for HeapStr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq for HeapStr {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for HeapStr {}

impl std::ops::Deref for HeapStr {
    type Target = str;
    #[inline]
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for HeapStr {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<RuntimeString> for HeapStr {
    fn from(s: RuntimeString) -> Self {
        HeapStr::shared(s)
    }
}
