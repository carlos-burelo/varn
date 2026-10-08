use std::rc::Rc;
use std::sync::Arc;
use varn_types::RuntimeString;

pub(crate) mod ascii_flag {
    pub const UNKNOWN: u8 = 0;
    pub const YES: u8 = 1;
    pub const NO: u8 = 2;
}

pub const INLINE_STR_CAP: usize = 37;

#[derive(Clone)]
pub enum HeapStr {
    Shared(RuntimeString, std::cell::Cell<u8>),
    Ext {
        buf: Rc<std::cell::UnsafeCell<String>>,
        len: usize,
        ascii: std::cell::Cell<u8>,
    },

    Inline {
        len: u8,
        ascii: std::cell::Cell<u8>,
        bytes: [u8; INLINE_STR_CAP],
    },
}

impl HeapStr {
    #[inline]
    pub(crate) fn shared(s: RuntimeString) -> Self {
        let flag = if s.is_ascii() {
            ascii_flag::YES
        } else {
            ascii_flag::NO
        };
        HeapStr::Shared(s, std::cell::Cell::new(flag))
    }

    #[inline]
    pub(crate) fn ext(buf: Rc<std::cell::UnsafeCell<String>>, len: usize, ascii: u8) -> Self {
        HeapStr::Ext {
            buf,
            len,
            ascii: std::cell::Cell::new(ascii),
        }
    }

    #[inline]
    pub(crate) fn inline(s: &str) -> Self {
        debug_assert!(s.len() <= INLINE_STR_CAP);
        let mut bytes = [0u8; INLINE_STR_CAP];
        bytes[..s.len()].copy_from_slice(s.as_bytes());
        let flag = if s.is_ascii() {
            ascii_flag::YES
        } else {
            ascii_flag::NO
        };
        HeapStr::Inline {
            len: s.len() as u8,
            ascii: std::cell::Cell::new(flag),
            bytes,
        }
    }

    #[inline]
    pub(crate) fn as_str(&self) -> &str {
        match self {
            HeapStr::Shared(s, _) => s,

            HeapStr::Ext { buf, len, .. } => unsafe { &(&*buf.get())[..*len] },

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
                self.set_ascii_state(if is { ascii_flag::YES } else { ascii_flag::NO });
                is
            }
        }
    }

    #[inline]
    pub(crate) fn is_ascii_cached(&self) -> bool {
        self.ascii_state() == ascii_flag::YES
    }

    #[inline]
    pub(crate) fn byte_len(&self) -> usize {
        match self {
            HeapStr::Shared(s, _) => s.len(),
            HeapStr::Ext { len, .. } => *len,
            HeapStr::Inline { len, .. } => *len as usize,
        }
    }

    #[inline]
    pub(crate) fn char_len(&self) -> usize {
        if self.is_ascii() {
            self.byte_len()
        } else {
            self.as_str().chars().count()
        }
    }

    #[inline]
    pub(crate) fn to_shared(&self) -> RuntimeString {
        match self {
            HeapStr::Shared(s, _) => Arc::clone(s),
            HeapStr::Ext { buf, len, .. } => {
                let slice = unsafe { &(&*buf.get())[..*len] };
                Arc::from(slice)
            }
            HeapStr::Inline { len, bytes, .. } => {
                let slice = unsafe { std::str::from_utf8_unchecked(&bytes[..*len as usize]) };
                Arc::from(slice)
            }
        }
    }

    #[inline]
    fn set_ascii_state(&self, state: u8) {
        match self {
            HeapStr::Shared(_, ascii) => ascii.set(state),
            HeapStr::Ext { ascii, .. } => ascii.set(state),
            HeapStr::Inline { ascii, .. } => ascii.set(state),
        }
    }

    #[inline]
    pub(crate) fn is_tip(&self) -> bool {
        match self {
            HeapStr::Ext { buf, len, .. } => unsafe { (&*buf.get()).len() == *len },
            HeapStr::Shared(..) | HeapStr::Inline { .. } => false,
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
